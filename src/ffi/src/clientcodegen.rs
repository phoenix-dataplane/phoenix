// mimic the generated code of rust stub
// Manually writing all the generated code.

#![no_main]

use std::future::{poll_fn, IntoFuture};
use std::sync::Arc;
use std::task::Poll;
use std::thread;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use lazy_static::lazy_static;
use mrpc::stub::{ClientStub, NamedService};
use mrpc::{WRef, RRef};
use tokio::runtime::Builder;
use tokio::task;

use crate::incrementer_ffi::completeIncrement;

include!("typescodegen.rs");

lazy_static! {
    static ref SEND_CHANNEL: (Sender<ClientWork>, Receiver<ClientWork>) = unbounded();
    static ref CONNECT_COMPLETE_CHANNEL: (Sender<usize>, Receiver<usize>) = bounded(1);
}

#[cxx::bridge]
mod incrementer_ffi {
    extern "Rust" {
        type ValueRequest;
        type ValueReply;
        type IncrementerClient;

        fn new_value_request() -> Box<ValueRequest>;
        fn val(self: &ValueRequest) -> u64;
        fn set_val(self: &mut ValueRequest, val: u64);
        fn key(self: &ValueRequest, index: usize) -> u8;
        fn key_size(self: &ValueRequest) -> usize;
        fn set_key(self: &mut ValueRequest, index: usize, value: u8);
        fn add_foo(self: &mut ValueRequest, value: u8);

        fn new_value_reply() -> Box<ValueReply>;
        fn val(self: &ValueReply) -> u64;
        fn set_val(self: &mut ValueReply, val: u64);

        fn initialize();

        fn connect(dst: String) -> Box<IncrementerClient>;
        unsafe fn increment(self: &IncrementerClient, req: Box<ValueRequest>, callback: *mut i32);
    }

    // TODO(nikolabo): can we do callbacks through function pointers? current approach seems to couple client and codegen code too much
    unsafe extern "C++" {
        include!("ffi/include/receive.h");
        fn completeIncrement(reply: Box<ValueReply>);
    }
}

// CLIENT CODE

#[derive(Debug)]
pub struct IncrementerClient {
    client_handle: usize,
}

enum ClientWork {
    Connect(String),
    Increment(usize, Box<ValueRequest>, extern "C" fn(Box<ValueReply>)),
    // Increment(usize, Box<ValueRequest>)
}

fn initialize() {
    println!("initializing mrpc stub...");
    thread::spawn(|| {
        println!("runtime thread started");
        let runtime = Builder::new_current_thread().build().unwrap();
        runtime.block_on(inside_runtime());
    });
}

async fn inside_runtime() {
    let mut clients: std::vec::Vec<Arc<ClientStub>> = std::vec::Vec::new();
    println!("tokio current thread runtime starting...");

    task::LocalSet::new()
        .run_until(async move {
            poll_fn(|cx| {
                let v: Vec<ClientWork> = SEND_CHANNEL.1.try_iter().collect(); // TODO(nikolabo): client mapping stored in vector, handle is vector index, needs to be updated so clients can be deallocated

                if v.len() > 0 {
                    println!("runtime received something from channel")
                };

                for i in v {
                    match i {
                        ClientWork::Connect(dst) => {
                            clients.push(connect_inner(dst));
                            CONNECT_COMPLETE_CHANNEL.0.send(clients.len() - 1).unwrap();
                            println!("runtime sent connect completion");
                        }
                        ClientWork::Increment(handle, req, callback) => {
                            println!("Increment request received by runtime thread");
                            let stub = Arc::clone(&clients.get(handle).unwrap());
                            task::spawn_local(async move {
                                let reply = increment_inner(
                                    stub,
                                    req,
                                ).await;
                                
                                (callback)(Box::new(*(reply.unwrap())));        // pass a reference count to rref or expect user to only use reference inside callback
                                // completeIncrement(Box::new(*reply.unwrap()));
                            });
                        }
                    }
                }

                cx.waker().wake_by_ref();
                Poll::Pending
            })
            .await
        })
        .await
}

fn connect(dst: String) -> Box<IncrementerClient> {
    // TODO(nikolabo): connect panics on error
    SEND_CHANNEL.0.send(ClientWork::Connect(dst)).unwrap();
    Box::new(IncrementerClient {
        client_handle: CONNECT_COMPLETE_CHANNEL.1.recv().unwrap(),
    })
}

fn connect_inner(dst: String) -> Arc<ClientStub> {
    // Force loading/reloading protos at the backend
    println!("connection starting...");
    update_protos().unwrap();

    let stub = ClientStub::connect(dst).unwrap();
    println!("phoenix backend connection established");
    Arc::clone(&stub)
}

fn update_protos() -> Result<(), ::mrpc::Error> {
    let srcs = [include_str!(
        "../../../src/phoenix_examples/proto/rpc_int/rpc_int.proto"
    )];
    ::mrpc::stub::update_protos(srcs.as_slice())
}

impl IncrementerClient {
    fn increment(&self, req: Box<ValueRequest>, callback: *mut i32) {
        let intermediate = callback as *const ();
        let callbackfn: extern "C" fn(Box<ValueReply>) = unsafe { std::mem::transmute(intermediate) };
        SEND_CHANNEL
            .0
            .send(ClientWork::Increment(self.client_handle, req, callbackfn))
            .unwrap();
        println!("Increment request sent to runtime thread...");
    }
}

fn increment_inner(
    stub: Arc<ClientStub>,
    req: Box<ValueRequest>,
) -> impl std::future::Future<Output = Result<mrpc::RRef<ValueReply>, ::mrpc::Status>> {
    let call_id = stub.initiate_call();
    // Fill this with the right func_id
    let func_id = 3784353755;

    let r = WRef::new(*req); // TODO(nikolabo): Rust stub only writes RPC data once, directly to shm heap, we introduce an extra copy here, how to avoid?

    stub.unary(IncrementerClient::SERVICE_ID, func_id, call_id, r)
}

impl NamedService for IncrementerClient {
    const SERVICE_ID: u32 = 2056765301;
    const NAME: &'static str = "rpc_int.Incrementer";
}
