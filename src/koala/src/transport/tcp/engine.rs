use std::collections::VecDeque;
use std::future::Future;
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

use interface::{AsHandle, Handle};
use ipc::customer::Customer;
use ipc::transport::tcp::{cmd, dp};

use interface::engine::SchedulingMode;

use super::module::CustomerType;
use super::{DatapathError, Error};
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;
use crate::resource::ResourceTable;

pub(crate) struct State {
    listener_table: ResourceTable<TcpListener>,
    conn_table: ResourceTable<TcpStream>,
}

impl State {
    pub(crate) fn new() -> Self {
        State {
            listener_table: ResourceTable::default(),
            conn_table: ResourceTable::default(),
        }
    }
}

pub struct TransportEngine {
    pub(crate) customer: CustomerType,
    pub(crate) node: Node,

    pub(crate) cq_err_buffer: VecDeque<dp::Completion>,
    pub(crate) _mode: SchedulingMode,

    pub(crate) state: State,

    // bufferred control path request
    pub(crate) cmd_buffer: Option<cmd::Command>,
    pub(crate) indicator: Option<Indicator>,
}

crate::unimplemented_ungradable!(TransportEngine);
crate::impl_vertex_for_engine!(TransportEngine, node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for TransportEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("TCP TransportEngine, user pid: update me")
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }
}

impl TransportEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            if let Status::Progress(n) = self.check_dp()? {
                nwork += n;
            }

            if self.customer.has_control_command() {
                self.flush_dp()?;
                if let Status::Disconnected = self.check_cmd()? {
                    return Ok(());
                }
            }

            if self.cmd_buffer.is_some() {
                self.check_cm_event()?;
            }

            self.indicator.as_ref().unwrap().set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl TransportEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.customer.try_recv_cmd() {
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(Error::InProgress) => {
                        // nothing to do, waiting for some network/device response
                        return Ok(Progress(0));
                    }
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::Disconnected) => Ok(Status::Disconnected),
            Err(ipc::TryRecvError::Other(_e)) => Err(Error::IpcTryRecv),
        }
    }

    fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use cmd::{Command, CompletionKind};
        match req {
            Command::Bind(addr) => {
                let listener = TcpListener::bind(addr).map_err(Error::Socket)?;
                // listener.set_nonblocking(true)?.map_err(Error::Socket)?;
                let handle = listener.as_raw_fd().as_handle();
                self.state
                    .listener_table
                    .open_or_create_resource(handle, listener);
                Ok(CompletionKind::Bind(handle))
            }
            Command::Accept(handle) => {
                let listener = self.state.listener_table.get(handle)?;
                let (sock, addr) = listener.accept().map_err(Error::Socket)?;
                sock.set_nonblocking(true).map_err(Error::Socket)?;
                let new_handle = sock.as_raw_fd().as_handle();
                self.state
                    .conn_table
                    .open_or_create_resource(new_handle, sock);
                Ok(CompletionKind::Accept(new_handle))
            }
            Command::Connect(addr) => {
                let sock = TcpStream::connect(addr).map_err(Error::Socket)?;
                sock.set_nonblocking(true).map_err(Error::Socket)?;
                let handle = sock.as_raw_fd().as_handle();
                self.state.conn_table.open_or_create_resource(handle, sock);
                Ok(CompletionKind::Connect(handle))
            }
            Command::SetSockOption(handle) => {
                unimplemented!("setsockoption");
                // Ok(CompletionKind::SetSockOption)
            }
        }
    }

    fn check_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cm_event(&mut self) -> Result<Status, Error> {
        unimplemented!();
    }
}
