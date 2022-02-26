use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransportType {
    Rdma,
    Socket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);