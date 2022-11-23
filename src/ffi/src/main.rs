#![no_main]
use std::net::SocketAddr;
use cxx::{CxxString};
use ipc::mrpc::cmd::Command;
use mrpc::MRPC_CTX;

#[cxx::bridge]
mod memfds {
    pub struct RawFd {
        fd: i32,
    }
    extern "Rust" {
        fn recv_fds() -> Vec<RawFd>;
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

#[cxx::bridge]
mod ipc_bridge {

    pub struct HandleBridge {
        id: u64,
    }

    pub struct ReadHeapRegionBridge {
        handle: HandleBridge,
        addr: usize,
        len: usize,
        file_off: i64,
    }

    pub struct ConnectResponseBridge {
        conn_handle: HandleBridge,
        read_regions: Vec<ReadHeapRegionBridge>,
    }

    pub struct Vaddr {
        handle: HandleBridge,
        ptr: usize,
    }

    extern "Rust" {
        fn send_cmd_connect(addr: &CxxString);
        fn send_cmd_mapped_addrs(conn_handle: ConnectResponseBridge, vaddrs: Vec<Vaddr>);
    }
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

use ipc_bridge::{ConnectResponseBridge, Vaddr};
fn send_cmd_mapped_addrs(conn_handle: ConnectResponseBridge, vaddrs: Vec<Vaddr>) {

}


