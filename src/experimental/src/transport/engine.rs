use std::ffi::CString;
use std::ptr;

use ipc;
use ipc::cmd::{Request, Response};

use engine::{Engine, SchedulingMode, Upgradable, Version};

use rdma::rdmacm;

pub struct TransportEngine {
    tx: ipc::Sender<Response>,
    rx: ipc::Receiver<Request>,
    mode: SchedulingMode,
}

impl TransportEngine {
    pub fn new(
        tx: ipc::Sender<Response>,
        rx: ipc::Receiver<Request>,
        mode: SchedulingMode,
    ) -> Self {
        TransportEngine { tx, rx, mode }
    }
}

impl Upgradable for TransportEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }

    fn resume(&mut self) {
        unimplemented!();
    }
}

impl Engine for TransportEngine {
    fn init(&mut self) {
        unimplemented!();
    }

    fn run(&mut self) -> bool {
        match self.rx.try_recv() {
            // handle request
            Ok(req) => {
                match req {
                    Request::NewClient(..) => unreachable!(),
                    Request::Hello(number) => {
                        self.tx.send(Response::HelloBack(number)).unwrap();
                    }
                    Request::GetAddrInfo(node, service, hints) => {
                        trace!(
                            "node: {:?}, service: {:?}, hints: {:?}",
                            node,
                            service,
                            hints,
                        );
                        let hints = hints.map(rdmacm::AddrInfoHints::from);
                        let ai = rdmacm::AddrInfoIter::getaddrinfo(
                            node.as_deref(),
                            service.as_deref(),
                            hints.as_ref(),
                        );
                        let ai_vec: Result<Vec<_>, interface::Error> = ai.map_or_else(
                            |e| Err(interface::Error::GetAddrInfo(e.raw_os_error().unwrap())),
                            |ai| Ok(ai.map(|x| x.into()).collect()),
                        );
                        self.tx.send(Response::GetAddrInfo(ai_vec)).unwrap();
                    }
                    Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                        // do something with this
                        trace!(
                            "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                            ai,
                            pd_handle,
                            qp_init_attr
                        );
                    }
                }
                true
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                false
            }
            Err(ipc::TryRecvError::IpcError(e)) => {
                if matches!(e, ipc::IpcError::Disconnected) {
                    return true;
                }
                panic!("recv error: {:?}", e);
            }
        }
    }

    fn shutdown(&mut self) {
        unimplemented!();
    }

    fn enqueue(&self) {
        unimplemented!();
    }

    fn check_queue_len(&self) {
        unimplemented!();
    }
}
