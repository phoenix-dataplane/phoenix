#[derive(Debug, ::mrpc_derive::Message)]
pub struct Request {
    #[prost(string, tag="1")]
    pub customer_name: ::mrpc_marshal::shadow::String,
    #[prost(string, repeated, tag="2")]
    pub hotel_id: ::mrpc_marshal::shadow::Vec<::mrpc_marshal::shadow::String>,
    #[prost(string, tag="3")]
    pub in_date: ::mrpc_marshal::shadow::String,
    #[prost(string, tag="4")]
    pub out_date: ::mrpc_marshal::shadow::String,
    #[prost(int32, tag="5")]
    pub room_number: i32,
}
#[derive(Debug, ::mrpc_derive::Message)]
pub struct Result {
    #[prost(string, repeated, tag="1")]
    pub hotel_id: ::mrpc_marshal::shadow::Vec<::mrpc_marshal::shadow::String>,
}

// #[derive(Clone, PartialEq, ::prost::Message)]
// pub struct Request {
//     #[prost(shm_string, tag="1")]
//     pub customer_name: ::mrpc::alloc::String,
//     #[prost(shm_string, repeated, tag="2")]
//     pub hotel_id: ::mrpc::alloc::Vec<::mrpc::alloc::String>,
//     #[prost(shm_string, tag="3")]
//     pub in_date: ::mrpc::alloc::String,
//     #[prost(shm_string, tag="4")]
//     pub out_date: ::mrpc::alloc::String,
//     #[prost(int32, tag="5")]
//     pub room_number: i32,
// }
// #[derive(Clone, PartialEq, ::prost::Message)]
// pub struct Result {
//     #[prost(shm_string, repeated, tag="1")]
//     pub hotel_id: ::mrpc::alloc::Vec<::mrpc::alloc::String>,
// }
