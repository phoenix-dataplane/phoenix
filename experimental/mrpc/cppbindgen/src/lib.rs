// mimic the generated code of rust stub
// Manually writing all the generated code.

#![no_main]
#![feature(once_cell)]
use mrpc::alloc::Vec;
use mrpc::{WRef, RRef};

// TYPES

#[derive(Debug, Default, Clone)]
pub struct ValueRequest {
    pub val: u64,
    pub key: ::mrpc::alloc::Vec<u8>,
}

pub struct RValueRequest {
    inner: RRef<ValueRequest>,
}

// Is there a good way to expose a general WRef API across FFI rather than
// generating a wrapper per message type?
pub struct WValueRequest {
    inner: WRef<ValueRequest>,
}

#[no_mangle]
pub extern "C" fn new_wvaluerequest() -> *mut WValueRequest {
    // wrap wref in box return raw pointer (should later Box::from_raw to drop)
    let res = Box::new(WValueRequest {
        inner: WRef::new(ValueRequest {
            val: 0,
            key: Vec::new(),
        }),
    });

    Box::into_raw(res)
}

impl RValueRequest {
    #[no_mangle]
    pub extern "C" fn rvaluerequest_val(&self) -> u64 {
        self.inner.val
    }

    #[no_mangle]
    pub extern "C" fn rvaluerequest_key(&self, index: usize) -> u8 {
        self.inner.key[index]
    }

    #[no_mangle]
    pub extern "C" fn rvaluerequest_key_size(&self) -> usize {
        self.inner.key.len()
    }

    #[no_mangle]
    pub extern "C" fn rvaluerequest_drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self));
        }
    }
}

impl WValueRequest {
    #[no_mangle]
    pub extern "C" fn wvaluerequest_val(&self) -> u64 {
        self.inner.val
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_key(&self, index: usize) -> u8 {
        self.inner.key[index]
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_key_size(&self) -> usize {
        self.inner.key.len()
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_set_val(&mut self, val: u64) {
        let wr_mut = WRef::<ValueRequest>::get_mut(&mut self.inner);
        // Should always get the mutable reference, since we never clone the WRef
        // If the cpp user passes the opaque reference between threads concurrent
        // access could occur. Solve by implementing DerefMut on WRef.
        match wr_mut {
            Some(wr) => wr.val = val,
            None => panic!("failed to get mutable wref"),
        }
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_set_key(&mut self, index: usize, value: u8) {
        let wr_mut = WRef::<ValueRequest>::get_mut(&mut self.inner);
        match wr_mut {
            Some(wr) => wr.key[index] = value,
            None => panic!("failed to get mutable wref"),
        }
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_key_add_byte(&mut self, value: u8) {
        let wr_mut = WRef::<ValueRequest>::get_mut(&mut self.inner);
        match wr_mut {
            Some(wr) => wr.key.push(value),
            None => panic!("failed to get mutable wref"),
        }
    }

    #[no_mangle]
    pub extern "C" fn wvaluerequest_drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self));
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ValueReply {
    pub val: u64,
}

pub struct RValueReply {
    inner: RRef<ValueReply>,
}

pub struct WValueReply {
    inner: WRef<ValueReply>,
}

#[no_mangle]
pub extern "C" fn new_wvaluereply() -> *mut WValueReply {
    let res = Box::new(WValueReply {
        inner: WRef::new(ValueReply { val: 0 }),
    });

    Box::into_raw(res)
}

impl RValueReply {
    #[no_mangle]
    pub extern "C" fn rvaluereply_val(&self) -> u64 {
        self.inner.val
    }

    #[no_mangle]
    pub extern "C" fn rvaluereply_drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self));
        }
    }
}

impl WValueReply {
    #[no_mangle]
    pub extern "C" fn wvaluereply_val(&self) -> u64 {
        self.inner.val
    }

    #[no_mangle]
    pub extern "C" fn wvaluereply_set_val(&mut self, val: u64) {
        let wr_mut = WRef::<ValueReply>::get_mut(&mut self.inner);
        match wr_mut {
            Some(wr) => wr.val = val,
            None => panic!("failed to get mutable wref"),
        }
    }

    #[no_mangle]
    pub extern "C" fn wvaluereply_drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self));
        }
    }
}

// INCREMENTER CLIENT

pub mod incrementer_client {
    use super::*;
    use std::ffi::CStr;
    use std::future::poll_fn;
    use std::os::raw::c_char;
    use std::sync::{Arc, OnceLock};
    use std::task::Poll;
    use std::thread;

    use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
    use mrpc::stub::{ClientStub, NamedService};
    use mrpc::WRef;
    use tokio::runtime::Builder;
    use tokio::task;

    static SEND_CHANNEL: OnceLock<(Sender<ClientWork>, Receiver<ClientWork>)> = OnceLock::new();
    static CONNECT_COMPLETE_CHANNEL: OnceLock<(Sender<usize>, Receiver<usize>)> = OnceLock::new();

    // TODO:(nikolabo) How to share runtime thread between different client instances
    pub enum ClientWork {
        Connect(String),
        Increment(
            usize,
            WRef<ValueRequest>,
            extern "C" fn(*const RValueReply),
        ),
    }

    #[no_mangle]
    pub extern fn initialize() {
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
                    let v: Vec<ClientWork> = SEND_CHANNEL.get().unwrap().1.try_iter().collect(); // TODO(nikolabo): client mapping stored in vector, handle is vector index, needs to be updated so clients can be deallocated

                    if v.len() > 0 {
                        println!("runtime received something from channel")
                    };

                    for i in v {
                        match i {
                            ClientWork::Connect(dst) => {
                                clients.push(connect_inner(dst));
                                CONNECT_COMPLETE_CHANNEL
                                    .get()
                                    .unwrap()
                                    .0
                                    .send(clients.len() - 1)
                                    .unwrap();
                                println!("runtime sent connect completion");
                            }
                            ClientWork::Increment(handle, req, callback) => {
                                println!("Increment request received by runtime thread");
                                let stub = Arc::clone(&clients.get(handle).unwrap());
                                task::spawn_local(async move {
                                    let reply = increment_inner(&stub, req).await;

                                    let r = Box::new(RValueReply { inner: reply.unwrap() });        // put rref on heap and call callback with rref pointer as param
                                    (callback)(Box::into_raw(r));
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

    #[derive(Debug)]
    pub struct IncrementerClient {
        client_handle: usize,
    }

    #[no_mangle]
    pub extern fn incrementer_client_connect(dst: *const c_char) -> *mut IncrementerClient {
        // TODO(nikolabo): find good way to report errors?
        let dest;
        unsafe {
            dest = CStr::from_ptr(dst).to_string_lossy().into_owned();
        }
        CONNECT_COMPLETE_CHANNEL.get_or_init(|| bounded(1));
        SEND_CHANNEL
            .get_or_init(|| unbounded())
            .0
            .send(ClientWork::Connect(dest))
            .unwrap();
        Box::into_raw(Box::new(IncrementerClient {
            client_handle: CONNECT_COMPLETE_CHANNEL.get().unwrap().1.recv().unwrap(),
        }))
    }

    fn connect_inner(dst: String) -> Arc<ClientStub> {
        // Force loading/reloading protos at the backend
        println!("connection starting...");
        update_protos().unwrap();

        let stub = ClientStub::connect(dst).unwrap();
        println!("phoenix backend connection established");
        Arc::new(stub)          // does this need to be in an ARC?
    }

    fn update_protos() -> Result<(), ::mrpc::Error> {
        let srcs = [include_str!(
            "../../../../src/phoenix_examples/proto/rpc_int/rpc_int.proto"
        )];
        ::mrpc::stub::update_protos(srcs.as_slice())
    }

    impl IncrementerClient {
        #[no_mangle]
        pub extern fn increment(&self, req: &WValueRequest, callback: extern "C" fn(*const RValueReply)) {
            SEND_CHANNEL
                .get()
                .unwrap()
                .0
                .send(ClientWork::Increment(self.client_handle, req.inner.clone(), callback))
                .unwrap();
            println!("Increment request sent to runtime thread...");
        }
    }

    fn increment_inner(
        stub: &Arc<ClientStub>,
        req: WRef<ValueRequest>,
    ) -> impl std::future::Future<Output = Result<mrpc::RRef<ValueReply>, ::mrpc::Status>> + '_
    {
        let call_id = stub.initiate_call();
        // Fill this with the right func_id
        let func_id = 3784353755;

        stub.unary(IncrementerClient::SERVICE_ID, func_id, call_id, req)
    }

    impl NamedService for IncrementerClient {
        const SERVICE_ID: u32 = 2056765301;
        const NAME: &'static str = "rpc_int.Incrementer";
    }
}
