use std::cell::{Ref, RefMut};
use std::collections::VecDeque;
use std::io::{IoSlice, Read, Write};
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use phoenix_api::buf::Range;
use phoenix_api::net::{MappedAddrStatus, WcOpcode, WcStatus};
use phoenix_api::transport::tcp::dp;
use phoenix_api::{AsHandle, Handle};

use super::state::State;
use super::{ApiError, TransportError};

pub struct Ops {
    pub state: State,
}

impl Ops {
    pub(crate) fn new(state: State) -> Self {
        Self { state }
    }

    fn poll(&self) -> Ref<Poll> {
        self.state.poll.borrow()
    }

    fn poll_mut(&self) -> RefMut<Poll> {
        self.state.poll.borrow_mut()
    }

    #[allow(dead_code)]
    fn events(&self) -> Ref<Events> {
        self.state.events.borrow()
    }

    fn events_mut(&self) -> RefMut<Events> {
        self.state.events.borrow_mut()
    }
}

// Control path APIs
impl Ops {
    pub fn bind(&self, addr: &SocketAddr) -> Result<Handle, ApiError> {
        let mut listener = TcpListener::bind(*addr)?;
        let handle = listener.as_raw_fd().as_handle();

        self.poll().registry().register(
            &mut listener,
            // Token((handle.0 as usize) << 32),
            Token(handle.0 as _),
            Interest::READABLE,
        )?;
        self.state
            .listener_table
            .borrow_mut()
            .insert(handle, listener);
        Ok(handle)
    }

    pub fn connect(&self, addr: &SocketAddr) -> Result<Handle, ApiError> {
        let mut sock = TcpStream::connect(*addr)?;
        sock.set_nodelay(true)?;
        let handle = sock.as_raw_fd().as_handle();

        self.poll().registry().register(
            &mut sock,
            Token(handle.0 as _),
            Interest::READABLE | Interest::WRITABLE,
        )?;
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

    fn try_accept(&self, listener_handle: Handle) -> Result<Handle, ApiError> {
        let table = self.state.listener_table.borrow();
        let listener = table.get(&listener_handle).ok_or(ApiError::NotFound)?;
        let (mut sock, _addr) = listener.accept()?;
        sock.set_nodelay(true)?;
        let sock_handle = sock.as_raw_fd().as_handle();

        self.poll().registry().register(
            &mut sock,
            Token(sock_handle.0 as usize),
            Interest::READABLE | Interest::WRITABLE,
        )?;
        self.state
            .sock_table
            .borrow_mut()
            .insert(sock_handle, (sock, MappedAddrStatus::Unmapped));
        self.state
            .cq_table
            .borrow_mut()
            .insert(sock_handle, CompletionQueue::new());
        Ok(sock_handle)
    }

    pub fn accept(&self, _handle: Handle) -> Result<Handle, ApiError> {
        unimplemented!("accept");
    }
}

const MAGIC_BYTES: usize = std::mem::size_of::<u32>();
const HEADER_BYTES: usize = MAGIC_BYTES + std::mem::size_of::<u32>() + std::mem::size_of::<u64>();

fn send_vectored(sock: &mut TcpStream, bufs: &[IoSlice]) -> Result<usize, TransportError> {
    match sock.write_vectored(bufs) {
        Ok(n) => Ok(n),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
        Err(e) => Err(TransportError::Socket(e)),
    }
}

fn recv(sock: &mut TcpStream, buf: &mut [u8]) -> Result<usize, TransportError> {
    match sock.read(buf) {
        Ok(0) => Err(TransportError::Disconnected),
        Ok(n) => Ok(n),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
        Err(e) => Err(TransportError::Socket(e)),
    }
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
        if cq.send_tasks.len() == 1 {
            let mut sock_table = self.state.sock_table.borrow_mut();
            let (sock, _status) = sock_table
                .get_mut(&sock_handle)
                .ok_or(TransportError::NotFound)?;
            self.poll().registry().reregister(
                sock,
                Token(sock_handle.0 as usize),
                Interest::READABLE | Interest::WRITABLE,
            )?;
        }
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
    pub fn poll_cq(
        &self,
        _sock_handle: Handle,
        _wcs: &mut [dp::Completion],
    ) -> Result<(), TransportError> {
        unimplemented!("poll cq");
    }

    pub fn poll_io(
        &self,
        duration: Duration,
    ) -> Result<(Vec<Handle>, Vec<dp::Completion>), TransportError> {
        let mut conns = Vec::new();
        let mut wcs = Vec::new();
        // let mut io_res = Vec::new();
        self.poll_mut()
            .poll(&mut self.events_mut(), Some(duration))?;

        for event in self.events_mut().iter() {
            let Token(handle) = event.token();
            if self
                .state
                .listener_table
                .borrow()
                .contains_key(&Handle(handle as _))
            {
                let listener_handle = handle;
                if let Ok(handle) = self.try_accept(Handle(listener_handle as _)) {
                    conns.push(handle);
                }
            } else {
                let _res = (|| -> Result<(), TransportError> {
                    let sock_handle = Handle(handle as _);
                    let mut sock_table = self.state.sock_table.borrow_mut();
                    let (sock, status) = sock_table
                        .get_mut(&sock_handle)
                        .ok_or(TransportError::NotFound)?;
                    let mut cq_table = self.state.cq_table.borrow_mut();
                    let cq = cq_table
                        .get_mut(&sock_handle)
                        .ok_or(TransportError::NotFound)?;
                    let mut write_would_block = true;
                    let mut read_would_block = true;
                    if event.is_writable() {
                        write_would_block = cq.check_write(sock, &mut wcs);
                    }
                    if event.is_readable() {
                        read_would_block = if *status != MappedAddrStatus::Unmapped {
                            cq.check_read(sock, &mut wcs)
                        } else {
                            false
                        };
                    }
                    if !read_would_block {
                        self.poll().registry().reregister(
                            sock,
                            Token(sock_handle.0 as usize),
                            if cq.send_tasks.is_empty() {
                                Interest::READABLE
                            } else {
                                Interest::READABLE | Interest::WRITABLE
                            },
                        )?;
                    } else if !write_would_block {
                        // send_tasks must be empty
                        self.poll().registry().reregister(
                            sock,
                            Token(sock_handle.0 as usize),
                            Interest::READABLE,
                        )?;
                    }
                    Ok(())
                })();
            }
        }
        Ok((conns, wcs))
    }
}

#[derive(Debug)]
pub(crate) struct Task {
    wr_id: u64,
    sock_handle: Handle,
    opcode: WcOpcode,
    meta: [u8; HEADER_BYTES],
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
            meta: Task::get_meta(imm, buf.len),
            buf,
            imm,
            expected,
            offset,
            error: Ok(()),
        }
    }

    pub(crate) fn get_meta(imm: u32, len: u64) -> [u8; HEADER_BYTES] {
        let mut meta: [u8; HEADER_BYTES] = [0; HEADER_BYTES];
        unsafe {
            let magic: u32 = 2563;
            std::ptr::write(meta.as_mut_ptr() as *mut u32, magic);
            std::ptr::write(meta.as_mut_ptr().offset(4) as *mut u32, imm);
            std::ptr::write(meta.as_mut_ptr().offset(8) as *mut u64, len);
        }
        meta
    }

    pub(crate) fn get_io_vec(&self) -> Vec<IoSlice> {
        let mut io_vec = Vec::new();
        let mut buf_offest = self.offset as i64 - HEADER_BYTES as i64;
        if buf_offest < 0 {
            io_vec.push(IoSlice::new(&self.meta[self.offset..]));
            buf_offest = 0;
        }
        let buf = unsafe {
            std::slice::from_raw_parts(
                (self.buf.offset as i64 + buf_offest) as *const u8,
                (self.buf.len as i64 - buf_offest) as usize,
            )
        };
        io_vec.push(IoSlice::new(buf));
        io_vec
    }

    pub(crate) fn get_comp(&self) -> dp::Completion {
        dp::Completion {
            wr_id: self.wr_id,
            conn_id: self.sock_handle.0 as u64,
            opcode: self.opcode,
            status: match &self.error {
                Ok(_) => WcStatus::Success,
                Err(e) => WcStatus::Error(NonZeroU32::new(e.as_vendor_err()).unwrap()),
            },
            buf: self.buf,
            byte_len: self.offset - HEADER_BYTES,
            imm: self.imm,
        }
    }
}

pub struct CompletionQueue {
    send_tasks: VecDeque<Task>,
    recv_tasks: VecDeque<Task>,
}

impl Default for CompletionQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionQueue {
    pub fn new() -> Self {
        CompletionQueue {
            send_tasks: VecDeque::with_capacity(128),
            recv_tasks: VecDeque::with_capacity(128),
        }
    }

    pub fn check_write(&mut self, sock: &mut TcpStream, wcs: &mut Vec<dp::Completion>) -> bool {
        while !self.send_tasks.is_empty() {
            let task = self.send_tasks.front_mut().unwrap();

            if task.opcode != WcOpcode::Send {
                task.error = Err(TransportError::General("Invalid socket opcode".to_string()));
                wcs.push(task.get_comp());
                self.send_tasks.pop_front();
                continue;
            }

            loop {
                let io_vec = task.get_io_vec();
                match send_vectored(sock, &io_vec) {
                    Ok(n) => {
                        if n == 0 {
                            return true;
                        }
                        task.offset += n;
                        if task.offset == task.expected {
                            break;
                        }
                    }
                    Err(e) => {
                        task.error = Err(e);
                        break;
                    }
                }
            }

            // Error or finished, either case should be popped
            wcs.push(task.get_comp());
            self.send_tasks.pop_front();
        }
        false
    }

    pub fn check_read(&mut self, sock: &mut TcpStream, wcs: &mut Vec<dp::Completion>) -> bool {
        while !self.recv_tasks.is_empty() {
            let task = self.recv_tasks.front_mut().unwrap();

            if task.opcode != WcOpcode::Recv {
                task.error = Err(TransportError::General("Invalid socket opcode".to_string()));
                wcs.push(task.get_comp());
                self.recv_tasks.pop_front();
                continue;
            }

            if task.offset < HEADER_BYTES {
                loop {
                    match recv(sock, &mut task.meta[task.offset..HEADER_BYTES]) {
                        Ok(n) => {
                            if n == 0 {
                                return true;
                            }
                            task.offset += n;
                            if task.offset == HEADER_BYTES {
                                // TODO(lsh): Can check magic number here
                                let len = unsafe {
                                    std::ptr::read_unaligned(
                                        (task.meta.as_ptr().offset(8)) as *const u64,
                                    )
                                } as usize;
                                task.expected = HEADER_BYTES + len;
                                task.imm = unsafe {
                                    std::ptr::read_unaligned(
                                        (task.meta.as_ptr().offset(4)) as *const u32,
                                    )
                                };
                                if len > task.buf.len as _ {
                                    task.error = Err(TransportError::General(
                                        "Insufficient recving buffer!".to_string(),
                                    ));
                                }
                                break;
                            }
                        }
                        Err(e) => {
                            task.error = Err(e);
                            break;
                        }
                    }
                }

                if task.error.is_err() {
                    wcs.push(task.get_comp());
                    self.recv_tasks.pop_front();
                    continue;
                }
            }

            loop {
                let buf = unsafe {
                    std::slice::from_raw_parts_mut(
                        (task.buf.offset as usize + task.offset - HEADER_BYTES) as _,
                        task.expected as usize - task.offset,
                    )
                };
                match recv(sock, buf) {
                    Ok(n) => {
                        if n == 0 {
                            return true;
                        }
                        task.offset += n;
                        if task.offset == task.expected {
                            break;
                        }
                    }
                    Err(e) => {
                        task.error = Err(e);
                        break;
                    }
                }
            }
            wcs.push(task.get_comp());
            self.recv_tasks.pop_front();
        }
        false
    }
}
