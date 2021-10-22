use ipc;
use ipc::cmd::{Request, Response};

use engine::{Engine, SchedulingMode, Upgradable, Version};

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
                    Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                        // do something with this
                        eprintln!(
                            "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                            ai, pd_handle, qp_init_attr
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
