use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, uapi::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
