///  The request message containing the user's name.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HelloRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub name: ::mrpc_marshal::shadow::Vec<u8>,
}
///  The response message containing the greetings
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HelloReply {
    #[prost(bytes = "vec", tag = "1")]
    pub message: ::mrpc_marshal::shadow::Vec<u8>,
}
