use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, phoenix_api::Error>;

/// A list of supported underlying transports to use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TransportType {
    #[serde(alias = "Rdma")]
    Rdma,
    #[default]
    #[serde(alias = "Tcp")]
    Tcp,
}

impl std::str::FromStr for TransportType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "RDMA" => Ok(Self::Rdma),
            "TCP" => Ok(Self::Tcp),
            _ => Err("Expect RDMA or TCP"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);

/// mRPC service setting.
///
/// Each user thread can have its own service setting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Setting {
    /// The transport to use.
    pub transport: TransportType,
    /// The NIC to use. TODO(cjr): choose a better configuration option rather than explicitly
    /// seting the NIC.
    pub nic_index: usize,
    /// The core index the user binds to. [`None`] if the user thread does not explicit set a
    /// scheduling affinity.
    pub core_id: Option<usize>,

    pub module_config: Option<String>,
}
