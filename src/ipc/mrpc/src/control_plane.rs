use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, interface::Error>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TransportType {
    #[default]
    #[serde(alias = "Rdma")]
    Rdma,
    #[serde(alias = "Tcp")]
    Tcp,
}

impl std::str::FromStr for TransportType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "RDMA" => Ok(Self::Rdma),
            "TCP" => Ok(Self::Tcp),
            _ => Err("Expect RDMA or TCP")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);

/// Service setting. Each use thread can have its own service setting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Setting {
    /// The transport to use.
    pub transport: TransportType,
    /// The NIC to use. TODO(cjr): choose a better configuration option rather than explicitly
    /// seting the NIC.
    pub nic_index: usize,
    /// Set when the user thread binds to a CPU core.
    pub core_id: Option<usize>,
}