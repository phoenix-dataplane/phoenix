use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewConfig(),

    // We can add more configuration options here
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
