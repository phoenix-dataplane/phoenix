use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportType {
    #[serde(alias = "Rdma")]
    Rdma,
    #[serde(alias = "Tcp")]
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);

/// Service setting. Each use thread can have its own service setting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Setting {
    /// The transport to use.
    pub transport: TransportType,
    /// The NIC to use. TODO(cjr): choose a better configuration option rather than explicitly
    /// seting the NIC.
    pub nic_index: usize,
    /// Set when the user thread binds to a CPU core.
    pub core_id: Option<usize>,
}

impl Default for Setting {
    fn default() -> Self {
        Setting {
            transport: TransportType::Rdma,
            nic_index: 0,
            core_id: None,
        }
    }
}
