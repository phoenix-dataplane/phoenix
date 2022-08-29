use crate::engine::manager::RuntimeManager;
use crate::engine::EngineContainer;
use crate::node::Node;
use crate::scheduler::engine::SchedulerEngine;
use interface::engine::EngineType;
use interface::engine::SchedulingMode::Dedicate;
use nix::unistd::Pid;
use std::sync::{Arc, Mutex};

pub(crate) struct SchedulerModule {
    scheduler_node: Arc<Mutex<Node>>,
}

impl SchedulerModule {
    pub fn new(runtime_manager: &Arc<RuntimeManager>) -> Self {
        // todo(xyc): not sure if _pid set to 0_ and _error handling_ are ok, requiring further consideration
        // let ops =
        //     match crate::transport::rdma::module::create_ops(runtime_manager, Pid::from_raw(0)) {
        //         Ok(ops) => ops,
        //         Err(e) => panic!("{}", e),
        //     };

        let node = Node::new(EngineType::Scheduler);
        // let engine = SchedulerEngine::new(node, Dedicate, ops);
        // let node_ref = engine.node.clone();
        // runtime_manager.submit(EngineContainer::new(engine), Dedicate);
        SchedulerModule {
            scheduler_node: Arc::new(Mutex::new(node)),
        }
    }

    pub fn get_scheduler_node(&self) -> &Arc<Mutex<Node>> {
        &self.scheduler_node
    }
}
