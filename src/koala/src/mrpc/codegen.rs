#[derive(koala_derive::Message, Debug)]
pub struct HelloRequest {
    #[prost(bytes, tag = "1")]
    pub name: crate::mrpc::shadow::vec::Vec<u8>,
}

#[derive(koala_derive::Message, Debug)]
pub struct HelloReply {
    #[prost(bytes, tag = "1")]
    pub name: crate::mrpc::shadow::vec::Vec<u8>,
}
