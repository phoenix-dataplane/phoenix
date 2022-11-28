#![no_main]
use std::net::SocketAddr;
use cxx::{CxxString};
use ipc::mrpc::cmd::{Command, Completion, CompletionKind};
use memfds::ReadHeapRegionBridge;
use mrpc::MRPC_CTX;
use ipc::mrpc::cmd::ReadHeapRegion;

#[cxx::bridge]
mod memfds {
    pub struct RawFd {
        fd: i32,
    }

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
        regions: Vec<ReadHeapRegionBridge>,
    }

    pub struct CompletionMappedAddrs {
        success: bool,
    }

    extern "Rust" {
        fn recv_fds() -> Vec<RawFd>;
        fn send_cmd_connect(addr: &CxxString);
        fn recv_comp_connect() -> CompletionConnect;
        //fn send_cmd_mapped_addrs(conn_handle: HandleBridge, vaddrs: Vec<Vaddr>);
        //fn recv_comp_mapped_addrs() -> CompletionMappedAddrs;
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

fn recv_comp_connect() -> memfds::CompletionConnect {
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
                        memfds::CompletionConnect {
                            success: true,
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

fn read_heap_region_bridge(region: &ReadHeapRegion) -> memfds::ReadHeapRegionBridge {
    return memfds::ReadHeapRegionBridge { 
        handle: memfds::HandleBridge { id: region.handle.0 }, 
        remote_addr: region.addr, 
        nbytes: region.len, 
        file_off: region.file_off, 
    }
}

fn construct_on_error() -> memfds::CompletionConnect {
    memfds::CompletionConnect{
        success: false,
        regions: Vec::new(),
    }
}

//fn send_cmd_mapped_addrs(conn_handle: ipc_bridge::HandleBridge, vaddrs: Vec<ipc_bridge::Vaddr>) {
//
//}

//fn recv_comp_mapped_addrs() -> ipc_bridge::CompletionMappedAddrs{
//    // rx_recv_impl!(ctx.service, CompletionKind::NewMappedAddrs)?;
//}


