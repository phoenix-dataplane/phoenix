use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};

use anyhow::anyhow;
use anyhow::Result;
use minstant::Instant;

use interface::engine::EngineType;
use ipc;
use ipc::transport::rdma::control_plane;
use ipc::unix::DomainSocket;

use super::engine::RateLimitEngine;
use crate::config::RateLimitConfig;
use crate::node::Node;

pub(crate) struct RateLimitEngineBuilder {
    node: Node,
    config: RateLimitConfig,
}

impl RateLimitEngineBuilder {
    fn new(node: Node, config: RateLimitConfig) -> Self {
        RateLimitEngineBuilder { node, config }
    }

    fn build(self) -> Result<RateLimitEngine> {
        assert_eq!(self.node.engine_type, EngineType::RateLimit);

        Ok(RateLimitEngine {
            node: self.node,
            indicator: Default::default(),
            requests_per_sec: self.config.requests_per_sec,
            last_ts: Instant::now(),
            num_tokens: 0,
            queue: VecDeque::new(),
        })
    }
}

pub struct RateLimitModule;

impl RateLimitModule {
    #[allow(dead_code)]
    pub fn handle_request(
        &mut self,
        // NOTE(cjr): Why I am using rdma's control_plane request
        req: &control_plane::Request,
        _sock: &DomainSocket,
        sender: &SocketAddr,
        _cred: &UCred,
    ) -> Result<()> {
        let _client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        match req {
            _ => unreachable!("unknown req: {:?}", req),
        }
    }

    pub(crate) fn create_engine(n: Node, config: RateLimitConfig) -> Result<RateLimitEngine> {
        let builder = RateLimitEngineBuilder::new(n, config);
        let engine = builder.build()?;

        Ok(engine)
    }
}
