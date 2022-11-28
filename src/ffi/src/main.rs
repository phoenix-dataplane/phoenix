#![no_main]
use std::net::SocketAddr;
use cxx::{CxxString};
use interface::Handle;
use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc_bridge::*;
use mrpc::MRPC_CTX;
use ipc::mrpc::cmd::ReadHeapRegion;

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

    pub struct Vaddr {
        handle: HandleBridge,
        ptr: usize,
    }

    pub struct ReadHeapRegionBridge {
        handle: HandleBridge,
        remote_addr: usize,
        nbytes: usize,
        file_off: i64,
    }

    pub struct CompletionConnect{
        success: bool,
        conn_handle: HandleBridge,
        regions: Vec<ReadHeapRegionBridge>,
    }

    pub struct CompletionMappedAddrs {
        success: bool,
    }

    extern "Rust" {
        fn send_cmd_connect(addr: &CxxString);
        fn recv_comp_connect() -> CompletionConnect;
        fn send_cmd_mapped_addrs(conn_handle: HandleBridge, vaddrs: Vec<ReadHeapRegionBridge>);
        fn recv_comp_mapped_addrs() -> CompletionMappedAddrs;
    }
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

fn send_cmd_connect(addr: &CxxString) {
    let addr_as_str = match addr.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    
    let server: SocketAddr = match addr_as_str.parse() {
        Ok(s) => s,
        Err(_) => return,
    };

    let req = Command::Connect(server);

    MRPC_CTX.with(|ctx| {
        ctx.service.send_cmd(req);
    });
}

fn recv_comp_connect() -> CompletionConnect {
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
                        CompletionConnect {
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

fn construct_on_error() -> CompletionConnect {
    CompletionConnect{
        success: false,
        conn_handle: HandleBridge {
            id: 0,
        },
        regions: Vec::new(),
    }
}

fn send_cmd_mapped_addrs(conn_handle: ipc_bridge::HandleBridge, regions: Vec<ReadHeapRegionBridge>) {
    let mut vaddrs: Vec<(Handle, usize)> = Vec::new();
    for region in regions {
        vaddrs.push((Handle {0: region.handle.id}, region.remote_addr))
    } 

    let req = Command::NewMappedAddrs(Handle {0: conn_handle.id}, vaddrs);

    MRPC_CTX.with(|ctx| {
        ctx.service.send_cmd(req);
    }); 
}

fn recv_comp_mapped_addrs() -> CompletionMappedAddrs{
    MRPC_CTX.with(|ctx| {
        match ctx.service.recv_comp() {
            Ok(comp) => {
                match &comp.0 {
                    Ok(CompletionKind::NewMappedAddrs) => CompletionMappedAddrs {success: true},
                    Err(_) => CompletionMappedAddrs {success: false},
                    _other => CompletionMappedAddrs {success: false},
                }
            } 
            Err(_) => CompletionMappedAddrs {success: false},
        } 
    }) 
}


