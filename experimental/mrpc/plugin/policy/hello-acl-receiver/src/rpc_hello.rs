///  The request message containing the user's name.
#[repr(C)]
#[derive(Debug, Clone, ::mrpc_derive::Message)]
pub struct HelloRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub name: ::mrpc_marshal::shadow::Vec<u8>,
}
///  The response message containing the greetings
#[repr(C)]
#[derive(Debug, ::mrpc_derive::Message)]
pub struct HelloReply {
    #[prost(bytes = "vec", tag = "1")]
    pub message: ::mrpc_marshal::shadow::Vec<u8>,
}

// ///  The request message containing the user's name.
// #[repr(C)]
// #[derive(Debug, Clone, ::mrpc_derive::Message)]
// pub struct HelloRequest {
//     #[prost(bytes = "vec", tag = "1")]
//     pub name: ::mrpc::alloc::Vec<u8>,
// }
// ///  The response message containing the greetings
// #[repr(C)]
// #[derive(Debug, ::mrpc_derive::Message)]
// pub struct HelloReply {
//     #[prost(bytes = "vec", tag = "1")]
//     pub message: ::mrpc::alloc::Vec<u8>,
// }
