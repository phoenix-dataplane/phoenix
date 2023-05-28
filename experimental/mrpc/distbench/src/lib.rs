// mimic the generated code of rust stub
// Manually writing all the generated code.

#![no_main]
#![feature(once_cell)]
use mrpc::alloc::Vec;
use mrpc::stub::LocalServer;
use mrpc::{WRef, RRef};

// TYPES

#[derive(Debug, Default, Clone)]
pub struct ValueRequest {
    pub val: u64,
    pub key: ::mrpc::alloc::Vec<u8>,
}

#[repr(transparent)]
pub struct RValueRequest {
    inner: RRef<ValueRequest>,
}

// Is there a good way to expose a general WRef API across FFI rather than
// generating a wrapper per message type?
#[repr(transparent)]
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
    use std::sync::{Arc};
    use std::task::Poll;
    use std::thread;

    use mrpc::stub::{ClientStub, NamedService};
    use mrpc::WRef;
    use tokio::runtime::Builder;
    use tokio::task;

    pub enum ClientWork {
        // Connect(String),
        Increment(
            WRef<ValueRequest>,
            extern "C" fn(*const RValueReply),
        ),
    }

    async fn inside_runtime(work_receiver: crossbeam_channel::Receiver<ClientWork>, connect_sender: crossbeam_channel::Sender<usize>, dest: String) {
        let stub = connect_inner(dest);
        connect_sender
            .send(0)
            .unwrap();

        task::LocalSet::new()
            .run_until(async move {
                poll_fn(|cx| {
                    let v: Vec<ClientWork> = work_receiver.try_iter().collect(); 

                    for c_work in v {
                        match c_work {
                            ClientWork::Increment(req, callback) => {
                                println!("Increment request received by runtime thread");
                                let s = Arc::clone(&stub);
                                task::spawn_local(async move {
                                    let reply = increment_inner(&s, req).await;

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
        client_sender: crossbeam_channel::Sender<ClientWork>,
    }

    #[no_mangle]
    pub extern fn incrementer_client_connect(dst: *const c_char) -> *mut IncrementerClient {
        // TODO(nikolabo): find good way to report errors?
        let dest;
        unsafe {
            dest = CStr::from_ptr(dst).to_string_lossy().into_owned();
        }
        let (work_sender, work_receiver) = crossbeam_channel::unbounded::<ClientWork>();
        let (connect_sender, connect_receiver) = crossbeam_channel::bounded::<usize>(1);

        thread::spawn(|| {
            let runtime = Builder::new_current_thread().build().unwrap();
            runtime.block_on(inside_runtime(work_receiver, connect_sender, dest));
        });

        connect_receiver.recv().unwrap();

        Box::into_raw(Box::new(IncrementerClient {
            client_sender: work_sender,
        }))
    }

    fn connect_inner(dst: String) -> Arc<ClientStub> {
        // Force loading/reloading protos at the backend
        update_protos().unwrap();

        let stub = ClientStub::connect(dst).unwrap();
        Arc::new(stub)
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
            self.client_sender
                .send(ClientWork::Increment(req.inner.clone(), callback))
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

#[no_mangle] 
pub extern "C" fn bind_mrpc_server(addr: *const std::os::raw::c_char) -> *mut LocalServer {
    let address;
        unsafe {
        address = std::ffi::CStr::from_ptr(addr).to_string_lossy().into_owned();
    }
    let server: std::net::SocketAddr = match address.parse() {
        Ok(s) => s,
        Err(_) => panic!("failed to connect"),
    };
    Box::into_raw(
        Box::new(
            mrpc::stub::LocalServer::bind(server).unwrap()
        )
    )
}

#[no_mangle]
pub extern "C" fn local_server_serve(l: &mut LocalServer) {
    smol::block_on(async {
        l.serve().await.unwrap();
    });
}

// INCREMENTER SERVER

pub mod incrementer_server {
    use super::*;
    use std::{net::SocketAddr, os::raw::c_char, ffi::CStr};
    use ::mrpc::stub::{NamedService, Service};

    // in original

    #[mrpc::async_trait]
    pub trait Incrementer: Send + Sync + 'static {
        /// increments an int
        async fn increment(
            &self,
            request: ::mrpc::RRef<super::ValueRequest>,
        ) -> Result<::mrpc::WRef<super::ValueReply>, ::mrpc::Status>;
    }

    #[derive(Debug)]
    pub struct IncrementerServer<T: Incrementer> {
        inner: T,
    }

    impl<T: Incrementer> IncrementerServer<T> {
        fn update_protos() -> Result<(), ::mrpc::Error> {
            let srcs = [include_str!(
                "../../../../src/phoenix_examples/proto/rpc_int/rpc_int.proto"
            )];
            ::mrpc::stub::update_protos(srcs.as_slice())
        }
        pub fn new(inner: T) -> Self {
            Self::update_protos().unwrap();
            Self { inner }
        }
    }

    impl<T: Incrementer> NamedService for IncrementerServer<T> {
        const SERVICE_ID: u32 = 2056765301u32;
        const NAME: &'static str = "rpc_int.Incrementer";
    }

    #[mrpc::async_trait]
    impl<T: Incrementer> Service for IncrementerServer<T> {
        async fn call(
            &self,
            req_opaque: ::mrpc::MessageErased,
            read_heap: std::sync::Arc<::mrpc::ReadHeap>,
        ) -> (::mrpc::WRefOpaque, ::mrpc::MessageErased) {
            let func_id = req_opaque.meta.func_id;
            match func_id {
                3784353755u32 => {
                    let req = ::mrpc::RRef::new(&req_opaque, read_heap);
                    let res = self.inner.increment(req).await;
                    match res {
                        Ok(reply) => ::mrpc::stub::service_post_handler(reply, &req_opaque),
                        Err(_status) => {
                            todo!();
                        }
                    }
                }
                _ => {
                    todo!("error handling for unknown func_id: {}", func_id);
                }
            }
        }
    }
    // fn add_service<S>(server: LocalServer, S)

    // #[no_mangle]
    // pub extern "C" fn run(
    //     addr: *const c_char,
    //     service: CPPIncrementer,
    // ) -> u64 {
    //     let address;
    //     unsafe {
    //         address = CStr::from_ptr(addr).to_string_lossy().into_owned();
    //     }
    //     let server: SocketAddr = match address.parse() {
    //         Ok(s) => s,
    //         Err(_) => return 1,
    //     };
    //     smol::block_on(async {
    //         let mut server = mrpc::stub::LocalServer::bind(server).unwrap();
    //         server
    //             .add_service(IncrementerServer::new(service))
    //             .serve()
    //             .await
    //             .unwrap();
    //         0
    //     })
    // }

    #[no_mangle]
    pub unsafe extern "C" fn add_incrementer_service(l: &mut LocalServer, service: CPPIncrementer) {
        l.add_service(IncrementerServer::new(service));
    }

    #[repr(C)]
    pub struct CPPIncrementer {
        pub state: *mut std::ffi::c_void,
        pub increment_impl: extern "C" fn(*mut std::ffi::c_void, *mut RValueRequest) -> *mut WValueReply,
    }

    // The mut pointer is an opaque reference to the state of an incrementer service, 
    // and the service implementation is expected to synchronize access to this state
    unsafe impl Send for CPPIncrementer {}
    unsafe impl Sync for CPPIncrementer {}

    impl Default for CPPIncrementer {
        fn default() -> Self {
            todo!()
        }
    }

    #[mrpc::async_trait]
    impl Incrementer for CPPIncrementer {
        async fn increment(
            &self,
            request: RRef<ValueRequest>,
        ) -> Result<WRef<ValueReply>, mrpc::Status> {

            let req = Box::into_raw(Box::new(RValueRequest { inner: request}));
            let rep = (self.increment_impl)(self.state, req);

            Ok( unsafe { (*rep).inner.clone() })
        }
    }
}