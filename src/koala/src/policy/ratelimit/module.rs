use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use atomic::Atomic;
use minstant::Instant;

use interface::engine::EngineType;
use ipc;
use ipc::policy::ratelimit::control_plane;
use ipc::unix::DomainSocket;

use super::engine::RateLimitEngine;
use crate::config::RateLimitConfig;
use crate::node::Node;

pub(crate) struct RateLimitEngineBuilder {
    node: Node,
    config: Arc<Atomic<RateLimitConfig>>,
}

impl RateLimitEngineBuilder {
    fn new(node: Node, config: Arc<Atomic<RateLimitConfig>>) -> Self {
        RateLimitEngineBuilder { node, config }
    }

    fn build(self) -> Result<RateLimitEngine> {
        assert_eq!(self.node.engine_type, EngineType::RateLimit);

        Ok(RateLimitEngine {
            node: self.node,
            indicator: Default::default(),
            config: Arc::clone(&self.config),
            last_ts: Instant::now(),
            num_tokens: 0,
            queue: VecDeque::new(),
        })
    }
}

pub struct RateLimitModule {
    config: Arc<Atomic<RateLimitConfig>>,
}

impl RateLimitModule {
    pub(crate) fn new(config: RateLimitConfig) -> Self {
        RateLimitModule {
            config: Arc::new(Atomic::new(config)),
        }
    }

    #[allow(dead_code)]
    pub fn handle_request(
        &mut self,
        req: &control_plane::Request,
        _sock: &DomainSocket,
        sender: &SocketAddr,
        _cred: &UCred,
    ) -> Result<()> {
        let _client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        match req {
            control_plane::Request::NewRate(new_rate) => {
                log::info!("Updating the requests_per_sec to {}", *new_rate);
                assert!(Atomic::<RateLimitConfig>::is_lock_free());
                self.config.store(
                    RateLimitConfig {
                        requests_per_sec: *new_rate,
                    },
                    Ordering::Relaxed,
                );
            }
        }
        Ok(())
    }

    pub(crate) fn create_engine(&self, n: Node) -> Result<RateLimitEngine> {
        let builder = RateLimitEngineBuilder::new(n, Arc::clone(&self.config));
        let engine = builder.build()?;

        Ok(engine)
    }
}
