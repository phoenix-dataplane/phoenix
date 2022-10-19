//! Common date types for engine
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SchedulingMode {
    #[default]
    /// Schedule to a dedicated runtime
    Dedicate,
    /// Put engines to a shared compact runtime,
    Compact,
    /// Spread engines in a scheduling group to different runtimes
    Spread,
    /// Put scheduling groups of a service
    /// with the same set of engines
    /// from multiple client subscriptions
    /// to a shared runtime
    /// while limiting each runtime to have at most
    /// a specified number of scheduling groups
    /// e.g., this enables us to put mRPC engine and RpcAdapter engine
    /// from multiple user applications to the same runtime
    GroupShared(usize),
}

// NOTE(wyj): Bincode do not support tagged enum
// https://github.com/bincode-org/bincode/issues/272
// so we create this enum to let network operator
// to specify scheduling override policy from toml config
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", content = "spec")]
pub enum CustomSchedulingSpec {
    GroupShared(usize),
}

impl From<CustomSchedulingSpec> for SchedulingMode {
    fn from(spec: CustomSchedulingSpec) -> Self {
        match spec {
            CustomSchedulingSpec::GroupShared(x) => SchedulingMode::GroupShared(x),
        }
    }
}

/// The user submits this hint to the backend. The content of this hint is subject to change.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchedulingHint {
    /// User desired scheduling mode.
    pub mode: SchedulingMode,
    /// The numa node the user thread affinites to.
    pub numa_node_affinity: Option<u8>,
}
