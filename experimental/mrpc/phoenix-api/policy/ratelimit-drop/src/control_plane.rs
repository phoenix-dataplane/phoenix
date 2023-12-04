use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewConfig(u64, u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
