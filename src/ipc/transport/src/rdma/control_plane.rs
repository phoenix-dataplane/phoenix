use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    /// The NIC to use.
    pub nic_index: usize,
}

impl Default for Setting {
    fn default() -> Self {
        Self { nic_index: 0 }
    }
}
