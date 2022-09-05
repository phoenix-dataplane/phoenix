use std::collections::VecDeque;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::os::unix::io::AsRawFd;

use interface::{AsHandle, Handle, MappedAddrStatus, WcOpcode, WcStatus};
use ipc::buf::Range;
use ipc::transport::tcp::dp;
use socket2::{Domain, Protocol, Socket, Type};

use super::state::State;
use super::{ApiError, TransportError};

pub struct Ops {
    pub state: State,
}

impl Ops {
    pub(crate) fn new(state: State) -> Self {
        Self { state }
    }
}

// Control path APIs
impl Ops {
    pub fn bind(&self, addr: &SocketAddr, backlog: i32) -> Result<Handle, ApiError> {
        let listener = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))
            .map_err(ApiError::Socket)?;
        listener.set_reuse_address(true).map_err(ApiError::Socket)?;
        listener.bind(&(*addr).into()).map_err(ApiError::Socket)?;
        listener.listen(backlog).map_err(ApiError::Socket)?;
        listener.set_nonblocking(true).map_err(ApiError::Socket)?;
        let handle = listener.as_raw_fd().as_handle();
        self.state
            .listener_table
            .borrow_mut()
            .insert(handle, listener);
        Ok(handle)
    }

    pub fn connect(&self, addr: &SocketAddr) -> Result<Handle, ApiError> {
        let sock = Socket::new(Domain::IPV4, Type::STREAM, None).map_err(ApiError::Socket)?;
        sock.connect(&(*addr).into()).map_err(ApiError::Socket)?;
        sock.set_nonblocking(true).map_err(ApiError::Socket)?;
        sock.set_nodelay(true).map_err(ApiError::Socket)?;
        let handle = sock.as_raw_fd().as_handle();
        self.state
            .sock_table
            .borrow_mut()
            .insert(handle, (sock, MappedAddrStatus::Mapped));
        self.state
            .cq_table
            .borrow_mut()
            .insert(handle, CompletionQueue::new());
        Ok(handle)
    }

    pub fn try_accept(&self) -> Vec<Handle> {
        let mut sock_handles = Vec::new();
        let mut table = self.state.listener_table.borrow_mut();
        let mut removed = Vec::new();
        for (_handle, listener) in table.iter_mut() {
            match listener.accept() {
                Ok((sock, _addr)) => {
                    if let Err(_e) = (|| -> Result<(), std::io::Error> {
                        sock.set_nonblocking(true)?;
                        sock.set_nodelay(true)?;
                        Ok(())
                    })() {
                        continue;
                    };

                    let sock_handle = sock.as_raw_fd().as_handle();
                    sock_handles.push(sock_handle);
                    self.state
                        .sock_table
                        .borrow_mut()
                        .insert(sock_handle, (sock, MappedAddrStatus::Unmapped));
                    self.state
                        .cq_table
                        .borrow_mut()
                        .insert(sock_handle, CompletionQueue::new());
                    removed.push(*_handle);
                }
                Err(_e) => continue,
            }
        }
        for handle in removed {
            table.remove(&handle);
        }
        sock_handles
    }

    pub fn accept(&self, handle: Handle) -> Result<Handle, ApiError> {
        let table = self.state.listener_table.borrow_mut();
        let listener = table.get(&handle).ok_or(ApiError::NotFound)?;
        let (sock, _addr) = listener.accept().map_err(ApiError::Socket)?;
        sock.set_nonblocking(true).map_err(ApiError::Socket)?;
        let handle = sock.as_raw_fd().as_handle();
        self.state
            .sock_table
            .borrow_mut()
            .insert(handle, (sock, MappedAddrStatus::Irrelevant));
        self.state
            .cq_table
            .borrow_mut()
            .insert(handle, CompletionQueue::new());
        Ok(handle)
    }
}

const MAGIC_BYTES: usize = std::mem::size_of::<u32>();
const HEADER_BYTES: usize = MAGIC_BYTES + std::mem::size_of::<u32>() + std::mem::size_of::<u64>();

fn send(sock: &Socket, buf: &[u8]) -> Result<usize, TransportError> {
    match sock.send(buf) {
        Ok(n) => Ok(n),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
        Err(e) => Err(TransportError::Socket(e)),
    }
}

fn recv(sock: &Socket, buf: &mut [u8]) -> Result<usize, TransportError> {
    let buf = unsafe { &mut *(buf as *mut [u8] as *mut [std::mem::MaybeUninit<u8>]) };
    match sock.recv(buf) {
        Ok(0) => Err(TransportError::Disconnected),
        Ok(n) => Ok(n),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
        Err(e) => Err(TransportError::Socket(e)),
    }
}

fn get_meta_buf(imm: u32, len: u64) -> [u8; 16] {
    let mut meta: [u8; 16] = [0; 16];
    unsafe {
        let magic: [u8; 4] = [0, 8, 2, 1];
        std::ptr::copy_nonoverlapping(magic.as_ptr(), meta.as_mut_ptr(), magic.len());
        std::ptr::copy_nonoverlapping(
            &imm as *const u32 as *const u8,
            meta.as_mut_ptr().offset(magic.len() as isize),
            std::mem::size_of::<u32>(),
        );
        std::ptr::copy_nonoverlapping(
            &len as *const u64 as *const u8,
            meta.as_mut_ptr()
                .offset((magic.len() + std::mem::size_of::<u32>()) as isize),
            std::mem::size_of::<u64>(),
        );
    }
    meta
}

// Data path APIs
impl Ops {
    /// Format:
    /// | magic | imm | len |      buf    |
    /// |   4   |  4  |  8  |   range.len |
    pub fn post_send(
        &self,
        sock_handle: Handle,
        wr_id: u64,
        range: Range,
        imm: u32,
    ) -> Result<(), TransportError> {
        let mut table = self.state.cq_table.borrow_mut();
        let cq = table
            .get_mut(&sock_handle)
            .ok_or(TransportError::NotFound)?;
        cq.send_tasks
            .push_back(Task::new(wr_id, sock_handle, WcOpcode::Send, range, imm, 0));
        Ok(())
    }

    pub fn post_recv(
        &self,
        sock_handle: Handle,
        wr_id: u64,
        range: Range,
    ) -> Result<(), TransportError> {
        let mut table = self.state.cq_table.borrow_mut();
        let cq = table
            .get_mut(&sock_handle)
            .ok_or(TransportError::NotFound)?;
        cq.recv_tasks
            .push_back(Task::new(wr_id, sock_handle, WcOpcode::Recv, range, 0, 0));
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn poll_cq(
        &self,
        sock_handle: Handle,
        wcs: &mut Vec<dp::Completion>,
    ) -> Result<(), TransportError> {
        let mut table = self.state.cq_table.borrow_mut();
        let cq = table
            .get_mut(&sock_handle)
            .ok_or(TransportError::NotFound)?;
        cq.poll(wcs);
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Task {
    wr_id: u64,
    sock_handle: Handle,
    opcode: WcOpcode,
    buf: Range,
    imm: u32,
    expected: usize, // expected include the header
    offset: usize,   // offset include the header
    error: Result<(), TransportError>,
}

impl Task {
    pub(crate) fn new(
        wr_id: u64,
        sock_handle: Handle,
        opcode: WcOpcode,
        buf: Range,
        imm: u32,
        offset: usize,
    ) -> Self {
        let expected = if opcode == WcOpcode::Recv {
            if offset >= HEADER_BYTES {
                let len = unsafe { std::ptr::read_unaligned((buf.offset as *const u64).offset(1)) };
                HEADER_BYTES + len as usize
            } else {
                0
            }
        } else {
            HEADER_BYTES + buf.len as usize
        };
        Task {
            wr_id,
            sock_handle,
            opcode,
            buf,
            imm,
            expected,
            offset: offset,
            error: Ok(()),
        }
    }
}

fn get_comp_from_task(task: &Task) -> dp::Completion {
    dp::Completion {
        wr_id: task.wr_id,
        conn_id: task.sock_handle.0 as u64,
        opcode: task.opcode,
        status: match &task.error {
            Ok(_) => WcStatus::Success,
            Err(e) => WcStatus::Error(NonZeroU32::new(e.into_vendor_err()).unwrap()),
        },
        buf: task.buf,
        byte_len: task.offset - HEADER_BYTES,
        imm: task.imm,
    }
}

pub struct CompletionQueue {
    cq: VecDeque<dp::Completion>,
    send_tasks: VecDeque<Task>,
    recv_tasks: VecDeque<Task>,
    start: std::time::Instant,
}

impl CompletionQueue {
    pub fn new() -> Self {
        CompletionQueue {
            cq: VecDeque::with_capacity(256),
            send_tasks: VecDeque::with_capacity(128),
            recv_tasks: VecDeque::with_capacity(128),
            start: std::time::Instant::now(),
        }
    }

    pub fn poll(&mut self, wcs: &mut Vec<dp::Completion>) {
        while !self.cq.is_empty() {
            wcs.push(*self.cq.front().unwrap());
            self.cq.pop_front();
        }
    }

    pub fn check_comp(&mut self, sock: &Socket, status: MappedAddrStatus) {
        loop {
            if let Some(task) = self.send_tasks.front_mut() {
                if task.opcode != WcOpcode::Send {
                    task.error = Err(TransportError::General("Invalid socket opcode".to_string()));
                }
                if task.error.is_err() {
                    self.cq.push_back(get_comp_from_task(task));
                    self.send_tasks.pop_front();
                    break;
                }

                if task.offset < HEADER_BYTES {
                    let meta = get_meta_buf(task.imm, task.buf.len);
                    match send(sock, &meta[task.offset..HEADER_BYTES]) {
                        Ok(n) => {
                            task.offset += n;
                            // koala::log::debug!(
                            //     "post_send head: \t{}\t{}\t\t{}",
                            //     task.wr_id,
                            //     n,
                            //     (std::time::Instant::now())
                            //         .duration_since(self.start)
                            //         .as_nanos() as f64
                            //         / 1000.0
                            // );
                            if task.offset < HEADER_BYTES {
                                break;
                            }
                        }
                        Err(e) => {
                            task.error = Err(e);
                            continue;
                        }
                    }
                }
                if task.offset < task.expected {
                    let buf = unsafe {
                        std::slice::from_raw_parts(
                            (task.buf.offset as usize + task.offset - HEADER_BYTES) as _,
                            task.buf.len as usize - task.offset + HEADER_BYTES,
                        )
                    };
                    match send(sock, buf) {
                        Ok(n) => {
                            task.offset += n;
                            // koala::log::debug!(
                            //     "post_send first:\t{}\t{}\t{}",
                            //     task.wr_id,
                            //     n,
                            //     (std::time::Instant::now())
                            //         .duration_since(self.start)
                            //         .as_nanos() as f64
                            //         / 1000.0
                            // );
                        }
                        Err(e) => {
                            task.error = Err(e);
                            continue;
                        }
                    }
                }
                if task.offset == task.expected {
                    self.cq.push_back(get_comp_from_task(task));
                    // koala::log::debug!(
                    //     "post_send comp:\t{}\t{:?}",
                    //     task.wr_id,
                    //     (std::time::Instant::now())
                    //         .duration_since(self.start)
                    //         .as_nanos() as f64
                    //         / 1000.0
                    // );
                    self.send_tasks.pop_front();
                }
            }
            break;
        }

        if status == MappedAddrStatus::Unmapped {
            return;
        }

        loop {
            if let Some(task) = self.recv_tasks.front_mut() {
                if task.opcode != WcOpcode::Recv {
                    task.error = Err(TransportError::General(
                        "Invalid socket opcode!".to_string(),
                    ));
                }
                if task.error.is_err() {
                    self.cq.push_back(get_comp_from_task(task));
                    self.recv_tasks.pop_front();
                    break;
                }

                if task.offset < HEADER_BYTES {
                    let buf = unsafe {
                        std::slice::from_raw_parts_mut(
                            (task.buf.offset as usize + task.offset) as _,
                            HEADER_BYTES - task.offset, //task.buf.len as usize - task.offset,
                        )
                    };
                    match recv(sock, buf) {
                        Ok(n) => {
                            task.offset += n;
                            // if n > 0 {
                            //     koala::log::debug!(
                            //         "post_recv head:\t{}\t{}\t{}",
                            //         task.wr_id,
                            //         n,
                            //         (std::time::Instant::now())
                            //             .duration_since(self.start)
                            //             .as_nanos() as f64
                            //             / 1000.0
                            //     );
                            // }

                            if task.offset >= HEADER_BYTES {
                                // TODO(lsh): Can check magic number here
                                let len = unsafe {
                                    std::ptr::read_unaligned(
                                        (task.buf.offset as *const u64).offset(1),
                                    ) as usize
                                };
                                task.expected = HEADER_BYTES + len;
                                task.imm = unsafe {
                                    std::ptr::read_unaligned(
                                        (task.buf.offset as *const u32).offset(1),
                                    )
                                };
                                if len > task.buf.len as _ {
                                    task.error = Err(TransportError::General(
                                        "Insufficient recving buffer!".to_string(),
                                    ));
                                }
                            } else {
                                break;
                            }
                        }
                        Err(e) => {
                            task.error = Err(e);
                            continue;
                        }
                    }
                }
                if task.offset < task.expected {
                    let buf = unsafe {
                        std::slice::from_raw_parts_mut(
                            (task.buf.offset as usize + task.offset - HEADER_BYTES) as _,
                            task.expected as usize - task.offset,
                        )
                    };
                    match recv(sock, buf) {
                        Ok(n) => {
                            task.offset += n;
                            // if n > 0 {
                            //     koala::log::debug!(
                            //         "post_recv first:\t{}\t{}\t{}",
                            //         task.wr_id,
                            //         n,
                            //         (std::time::Instant::now())
                            //             .duration_since(self.start)
                            //             .as_nanos() as f64
                            //             / 1000.0
                            //     );
                            // }
                        }
                        Err(e) => {
                            task.error = Err(e);
                            continue;
                        }
                    }
                }
                if task.offset == task.expected {
                    self.cq.push_back(get_comp_from_task(task));
                    // koala::log::debug!(
                    //     "post_recv comp:\t{}\t\t{}",
                    //     task.wr_id,
                    //     (std::time::Instant::now())
                    //         .duration_since(self.start)
                    //         .as_nanos() as f64
                    //         / 1000.0
                    // );
                    self.recv_tasks.pop_front();
                }
            }
            break;
        }
    }
}
