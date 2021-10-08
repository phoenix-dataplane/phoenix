use ipc;
use ipc::cmd::{Request, Response};

use crate::{
    engine::{Engine, Version},
    SchedulingMode,
};

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

impl Engine for TransportEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn init(&mut self) {
        unimplemented!();
    }

    fn run(&mut self) {
        let _x = self.rx.try_recv().unwrap();
        // handle request
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }

    fn destroy(&self) {
        unimplemented!();
    }

    fn enqueue(&self) {
        unimplemented!();
    }

    fn check_queue_len(&self) {
        unimplemented!();
    }
}
