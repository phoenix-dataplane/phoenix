//! Local I/O reactor fetch completions from the backend mRPC service and
//! dispatch the completions to corresponding completion queue.
use std::task::{Context, Poll};

use fnv::FnvHashMap as HashMap;
use slab::Slab;

use ipc::channel::{create_channel, ChannelFlavor, Receiver, Sender};
use phoenix_api::Handle;
use phoenix_api_mrpc::dp;

use super::conn::Connection;
use crate::{Error, MRPC_CTX};

#[derive(Debug)]
pub struct Reactor {
    senders: Slab<Sender<dp::Completion>>,
    buffer: Vec<dp::Completion>,
    // conn_id -> stub_id
    conn_to_stub: HashMap<Handle, usize>,
}

// After all, someone is going to do the mapping from conn_id to stub_id

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Reactor {
    pub fn new() -> Self {
        Reactor {
            senders: Slab::new(),
            buffer: Vec::with_capacity(32),
            conn_to_stub: HashMap::default(),
        }
    }

    /// Returns a stub_id and a Receiver for completion service.
    pub(crate) fn register_stub(&mut self) -> (usize, Receiver<dp::Completion>) {
        let (sender, receiver) = create_channel(ChannelFlavor::Sequential);
        let stub_id = self.senders.insert(sender);
        (stub_id, receiver)
    }

    pub(crate) fn register_connection(&mut self, stub_id: usize, conn: &Connection) {
        let conn_id = conn.handle();
        if let Some(h) = self.conn_to_stub.insert(conn_id, stub_id) {
            panic!("Duplicated conn_id: {:?}", h);
        }
    }

    pub fn poll(&mut self, _cx: &mut Context<'_>) -> Poll<Result<usize, Error>> {
        MRPC_CTX.with(|ctx| {
            // let has_work = futures::ready!(ctx.service.poll_wc_readable(cx))?;
            // if !has_work {
            //     return Poll::Pending;
            // }

            unsafe { self.buffer.set_len(0) };

            // read completions into a local buffer
            ctx.service
                .dequeue_wc_with(|ptr, count| unsafe {
                    for i in 0..count {
                        let c = ptr.add(i).cast::<dp::Completion>().read();
                        self.buffer.push(c);
                    }
                    count
                })
                .map_err(Error::Service)?;

            // let cnt = self.buffer.len();

            // dispatch newly arrived completions
            let ret = self.buffer.len();

            for c in &self.buffer {
                // get connection id
                let conn_id = match c {
                    dp::Completion::Incoming(msg) => msg.meta.conn_id,
                    dp::Completion::Outgoing(rpc_id, _status) => rpc_id.0,
                    dp::Completion::RecvError(conn_id, _status) => *conn_id,
                };

                // find the stub and push the completion to that stub
                let stub_id = self.conn_to_stub[&conn_id];
                let sender = self
                    .senders
                    .get_mut(stub_id)
                    .unwrap_or_else(|| panic!("unknown stub_id {}", stub_id));
                sender.send(c.clone()).unwrap();
            }

            Poll::Ready(Ok(ret))
        })
    }
}
