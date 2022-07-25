use std::fmt;
use std::time::Duration;

use minstant::Instant;

use interface::rpc::RpcId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SampleKind {
    ServerRequest = 1,
    ServerReply = 2,
    ClientRequest = 127,
    ClientReply = 128,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    ts: Instant,
    rpc_id: RpcId,
    kind: SampleKind,
}

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct SampleOutput {
    dura: Duration,
    rpc_id: RpcId,
    kind: SampleKind,
}

impl std::ops::Sub for Sample {
    type Output = SampleOutput;
    fn sub(self, rhs: Self) -> Self::Output {
        SampleOutput {
            dura: self.ts - rhs.ts,
            rpc_id: self.rpc_id,
            kind: self.kind,
        }
    }
}

impl Sample {
    #[inline]
    pub(crate) fn new(rpc_id: RpcId, kind: SampleKind) -> Self {
        Sample {
            ts: Instant::now(),
            rpc_id,
            kind,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Timer {
    samples: Vec<Sample>,
}

impl Timer {
    pub(crate) fn new() -> Self {
        Timer {
            samples: Vec::with_capacity(4096),
        }
    }

    #[inline]
    pub(crate) fn sample(&mut self, rpc_id: RpcId, kind: SampleKind) {
        self.samples.push(Sample::new(rpc_id, kind));
    }
}

impl fmt::Display for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (&s, &l) in self.samples.iter().skip(1).zip(&self.samples) {
            writeln!(f, "{:?}", s - l)?;
        }
        Ok(())
    }
}
