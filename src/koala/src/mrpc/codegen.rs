#[derive(mrpc_derive::Message, Debug)]
pub struct HelloRequest {
    #[prost(bytes, tag = "1")]
    pub name: mrpc_marshal::shadow::vec::Vec<u8>,
}

#[derive(mrpc_derive::Message, Debug)]
pub struct HelloReply {
    #[prost(bytes, tag = "1")]
    pub name: mrpc_marshal::shadow::vec::Vec<u8>,
}
