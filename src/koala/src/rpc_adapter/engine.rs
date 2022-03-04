use ipc::transport::tcp::{cmd, dp};

use engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use interface::engine::SchedulingMode;

use crate::node::Node;
use super::module::ServiceType;

pub struct RpcAdapterEngine {
    pub(crate) service: ServiceType,
    
    pub(crate) node: Node,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,
}

impl Upgradable for RpcAdapterEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }
}

impl Vertex for RpcAdapterEngine {
    crate::impl_vertex_for_engine!(node);
}

impl Engine for RpcAdapterEngine {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        unimplemented!();
    }
}
