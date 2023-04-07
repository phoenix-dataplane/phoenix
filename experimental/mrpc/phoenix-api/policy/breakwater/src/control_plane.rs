use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    // Get the current configuration [include the request_credits and request_timestamp as u64]
    NewConfig(u64, u64, u64, u64),

    // We can add more configuration options here
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);