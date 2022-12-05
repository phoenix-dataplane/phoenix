#![no_main]
use std::{net::SocketAddr, os::fd::AsRawFd};
use memfd::Memfd;
use std::io;
use alloc::AllocShmCompletionBridge;
use cxx::{CxxString};
use interface::{Handle, rpc::{MessageMeta, RpcMsgType}};
use ipc::mrpc::{cmd::{Command, CompletionKind}, dp::{self, WorkRequest}};
use ipc::salloc::cmd::Command as SallocCommand;
use ipc::salloc::cmd::CompletionKind as SallocCompletion;
use salloc::backend::SA_CTX;
use ipc_bridge::*;
use mrpc::{MRPC_CTX, MessageErased};
use ipc::mrpc::cmd::ReadHeapRegion;
use mmap::MmapFixed;

#[cxx::bridge]
mod memfds {
    pub struct RawFd {
        fd: i32,
    }
    extern "Rust" {
        fn recv_fds() -> Vec<RawFd>;
    }
}

#[cxx::bridge]
mod ipc_bridge {
    pub struct HandleBridge {
        id: u64,
    }

    pub struct CallIDBridge {
        id: u64,
    }

    pub enum RpcMsgTypeBridge {
        Request,
        Response,
    }

    pub struct VaddrBridge {
        handle: HandleBridge,
        ptr: usize,
    }

    // Stand in for a Result<(), Error)>, might need to expose mrpc::Error
    // or find better way to communicate this to the c++
    pub struct ResultBridge {
        success: bool,
    }

    pub struct ReadHeapRegionBridge {
        handle: HandleBridge,
        remote_addr: usize,
        nbytes: usize,
        file_off: i64,
    }

    pub struct CompletionConnectBridge {
        success: bool,
        conn_handle: HandleBridge,
        regions: Vec<ReadHeapRegionBridge>,
    }

    pub struct CompletionMappedAddrsBridge {
        success: bool,
    }

    pub struct MessageMetaBridge {
        pub conn_id: HandleBridge,
        pub service_id: u32,
        pub func_id: u32,
        pub call_id: CallIDBridge,
        pub token: u64,
        pub msg_type: RpcMsgTypeBridge,
    }

    pub struct MessageBridge {  // for messageErased
        pub meta: MessageMetaBridge,
        pub shm_addr_app: usize,
        pub shm_addr_backend: usize,
    }

    pub enum WorkRequestType {
        Call,
        Response,
        ReclaimRecvBuf,
    }

    pub struct WorkRequestBridge {  // TODO(nikolabo): support ReclaimRecvBuf wr
        pub wr_type: WorkRequestType,
        pub message: MessageBridge,
    }

    extern "Rust" {
        fn send_cmd_connect(addr: &CxxString) -> bool;
        fn recv_comp_connect() -> CompletionConnectBridge;
        fn send_cmd_mapped_addrs(conn_handle: HandleBridge, vaddrs: Vec<ReadHeapRegionBridge>) -> bool;
        fn recv_comp_mapped_addrs() -> CompletionMappedAddrsBridge;
        fn enqueue_wr(wr: WorkRequestBridge) -> ResultBridge;
    }
}

impl WorkRequestBridge {
    fn to_wr(&self) -> WorkRequest {
        let meta = MessageMeta {
            conn_id: interface::Handle(self.message.meta.conn_id.id),
            service_id: self.message.meta.service_id,
            func_id: self.message.meta.func_id,
            call_id: interface::rpc::CallId(self.message.meta.call_id.id),
            token: self.message.meta.token,
            msg_type: self.message.meta.msg_type.to_rmt(),
        };
        let erased = MessageErased {
            meta,
            shm_addr_app: self.message.shm_addr_app,
            shm_addr_backend: self.message.shm_addr_backend,
        };
        dp::WorkRequest::Call(erased)
    }
}

impl RpcMsgTypeBridge {
    fn to_rmt(self: RpcMsgTypeBridge) -> RpcMsgType {
        match self {
            RpcMsgTypeBridge::Request => RpcMsgType::Request,
            RpcMsgTypeBridge::Response => RpcMsgType::Response,
            _ => panic!("Unexpected invalid RpcMsgTypeBridge"),
        }
    }
}

#[cxx::bridge]
mod alloc {
    pub struct AllocShmCompletionBridge {
        success: bool,
        remote_addr: usize,
        file_off: i64,
        fd: i32,
    }

    extern "Rust" {
        fn allocate_shm(len: usize, align: usize) -> AllocShmCompletionBridge;
    }
}

fn allocate_shm(len: usize, align: usize) -> AllocShmCompletionBridge {
    let req = SallocCommand::AllocShm(len, align);

    SA_CTX.with(|ctx| {
        let res = match ctx.service.send_cmd(req) {
            Ok(_) => 1,
            Err(_) => 0,
        };

        if res == 0 {
            return AllocShmCompletionBridge {
                success: false,
                remote_addr: 0,
                file_off: 0,
                fd: 0,
            };
        }

        // todo: aman - figure out why recv_fds is blocking indefinitely here 
        let fds = ctx.service.recv_fd().unwrap();

        assert_eq!(fds.len(), 1);

        let memfd = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error()).unwrap();
        let file_len = memfd.as_file().metadata().unwrap().len() as usize;
        assert!(file_len >= len);
        println!("{}", fds[0]);

        match ctx.service.recv_comp().unwrap().0 {
            Ok(SallocCompletion::AllocShm(remote_addr, file_off)) => {
                match MmapFixed::new(remote_addr, len, file_off as i64, memfd.as_file()) {
                    Ok(_) => println!("valid mmap in rust"),
                    Err(_) => println!("failed mmap in rust"),
                }
                AllocShmCompletionBridge {
                    success: true,
                    remote_addr,
                    file_off,
                    fd: memfd.as_file().as_raw_fd(), 
                }
            }
            Err(e) => {
                println!("{}", e);
                AllocShmCompletionBridge { success: false, remote_addr: 0, file_off: 0, fd: 0 }
            },
            otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
        }
    })

}

fn recv_fds() -> Vec<memfds::RawFd> {
    MRPC_CTX.with(|ctx| {
        let mut fds = Vec::new();
        for fd in ctx.service.recv_fd().unwrap().iter() {
            fds.push(memfds::RawFd{fd: *fd});
        }
        fds
    })
}

fn send_cmd_connect(addr: &CxxString) -> bool {
    let addr_as_str = match addr.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    
    let server: SocketAddr = match addr_as_str.parse() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let req = Command::Connect(server);

    MRPC_CTX.with(|ctx| {
        match ctx.service.send_cmd(req) {
            Ok(_) => true,
            Err(_) => false,
        }
    })
}

fn recv_comp_connect() -> CompletionConnectBridge {
    MRPC_CTX.with(|ctx| {
        match ctx.service.recv_comp() {
            Ok(comp) => {
                match &comp.0 {
                    Ok(CompletionKind::Connect(conn_resp)) => {
                        // create and return CompletionConnect
                        let mut regions: Vec<ReadHeapRegionBridge> = Vec::new();
                        for region in conn_resp.read_regions.iter() {
                            regions.push(read_heap_region_bridge(region));
                        }
                        CompletionConnectBridge {
                            success: true,
                            conn_handle: HandleBridge {
                                id: conn_resp.conn_handle.0,
                            },
                            regions: regions,
                        }
                    },
                    Err(_) => construct_on_error(),
                    _other => construct_on_error(),
                }
            } 
            Err(_) => construct_on_error(),
        } 
    })
}

fn read_heap_region_bridge(region: &ReadHeapRegion) -> ReadHeapRegionBridge {
    return ReadHeapRegionBridge { 
        handle: HandleBridge { id: region.handle.0 }, 
        remote_addr: region.addr, 
        nbytes: region.len, 
        file_off: region.file_off, 
    }
}

fn construct_on_error() -> CompletionConnectBridge {
    CompletionConnectBridge {
        success: false,
        conn_handle: HandleBridge {
            id: 0,
        },
        regions: Vec::new(),
    }
}

fn send_cmd_mapped_addrs(conn_handle: ipc_bridge::HandleBridge, regions: Vec<ReadHeapRegionBridge>) -> bool {
    let mut vaddrs: Vec<(Handle, usize)> = Vec::new();
    for region in regions {
        vaddrs.push((Handle {0: region.handle.id}, region.remote_addr))
    } 

    let req = Command::NewMappedAddrs(Handle {0: conn_handle.id}, vaddrs);

    MRPC_CTX.with(|ctx| {
        match ctx.service.send_cmd(req) {
            Ok(_) => true,
            Err(_) => false,
        }
    }) 
}

fn recv_comp_mapped_addrs() -> CompletionMappedAddrsBridge {
    MRPC_CTX.with(|ctx| {
        match ctx.service.recv_comp() {
            Ok(comp) => {
                match &comp.0 {
                    Ok(CompletionKind::NewMappedAddrs) => CompletionMappedAddrsBridge {success: true},
                    Err(_) => CompletionMappedAddrsBridge {success: false},
                    _other => CompletionMappedAddrsBridge {success: false},
                }
            } 
            Err(_) => CompletionMappedAddrsBridge {success: false},
        } 
    }) 
}

fn enqueue_wr(wr: WorkRequestBridge) -> ResultBridge {
    MRPC_CTX.with(|ctx| {
        let mut sent = false;
        while !sent {
            let c = |ptr: *mut [u8; 64], _count: usize| unsafe {
                ptr.cast::<dp::WorkRequest>().write(wr.to_wr());
                sent = true;
                1
            };
            if let Err(_) = ctx.service.enqueue_wr_with(c) {
                return ResultBridge {success: false};
            }
        }
        ResultBridge {success: true}
    })
}