use clap::Parser;
use std::time::{Duration, Instant};

use phoenix_syscalls::verbs::{RemoteKey, SendFlags, WcStatus};
use phoenix_syscalls::Error;
use phoenix_syscalls::{cm, verbs};

pub const CTX_POLL_BATCH: usize = 16;

#[derive(Parser, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Send,
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    BW,
    LAT,
}

impl From<&str> for Verb {
    fn from(cmd: &str) -> Self {
        match cmd.to_lowercase().as_str() {
            "read" => Verb::Read,
            "write" => Verb::Write,
            _ => Verb::Send,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub verb: Verb,
    pub ip: String,
    pub port: u16,
    pub num: usize,
    pub warmup: usize,
    pub size: usize,
    pub num_qp: usize,
    pub num_client_threads: usize,
    pub num_server_threads: usize,
}

#[derive(Debug, Clone)]
pub struct Context {
    pub opt: Opts,
    pub tst: Test,
    pub client: bool,
    pub cap: verbs::QpCapability,
}

impl Context {
    pub fn new(mut opt: Opts, tst: Test) -> Self {
        opt.size = std::cmp::max(opt.size, 4);
        if tst == Test::BW && opt.num < opt.warmup {
            opt.num += opt.warmup;
        }

        let cap = verbs::QpCapability {
            max_send_wr: if tst == Test::BW { 128 } else { 1 },
            max_recv_wr: 512,
            max_send_sge: 1,
            max_recv_sge: 1,
            // max_inline_data: if tst == Test::BW { 0 } else { 236 },
            max_inline_data: 236,
        };

        Context {
            client: opt.ip != "0.0.0.0",
            opt,
            tst,
            cap,
        }
    }

    pub fn print(&self) {
        println!("machine: {}", if self.client { "client" } else { "sever" });
        println!(
            "num:{}, size:{}, warmup:{}",
            self.opt.num, self.opt.size, self.opt.warmup
        );
        match self.opt.verb {
            Verb::Send => println!("Send data from client to server"),
            Verb::Read => println!("Read data from server to client"),
            Verb::Write => println!("Write data from client to server"),
        }
    }
}

macro_rules! unsafe_write_bytes {
    ($ty:ty, $n:expr, $buf:expr) => {
        assert!(std::mem::size_of::<$ty>() <= $buf.len());
        unsafe {
            let bytes = *(&$n.to_be() as *const _ as *const [u8; std::mem::size_of::<$ty>()]);
            std::ptr::copy_nonoverlapping(
                (&bytes).as_ptr(),
                ($buf).as_mut_ptr(),
                std::mem::size_of::<$ty>(),
            );
        }
    };
}

macro_rules! unsafe_read_volatile {
    ($ty:ty,$addr:expr) => {
        <$ty>::from_be(unsafe { std::ptr::read_volatile($addr) })
    };
}

pub fn handshake(
    pre_id: cm::PreparedCmId,
    ctx: &Context,
    my_rkey: &RemoteKey,
) -> Result<(cm::CmId, RemoteKey), Error> {
    let mut send_mr: verbs::MemoryRegion<RemoteKey> = pre_id.alloc_msgs(1)?;
    let mut recv_mr: verbs::MemoryRegion<RemoteKey> = pre_id.alloc_msgs(1)?;

    send_mr[0] = *my_rkey;

    unsafe {
        pre_id
            .post_recv(&mut recv_mr, .., 0)
            .expect("Post recv failed!");
    }

    let id = if ctx.client {
        pre_id.connect(None)?
    } else {
        pre_id.accept(None)?
    };

    unsafe {
        id.post_send(&send_mr, .., 0, SendFlags::SIGNALED)?;
    }

    let wc = id.get_send_comp()?;
    assert_eq!(wc.status, WcStatus::Success);

    let wc = id.get_recv_comp()?;
    assert_eq!(wc.status, WcStatus::Success);

    Ok((id, recv_mr[0]))
}

const LAT_MEASURE_TAIL: usize = 2;
pub fn print_lat(ctx: &Context, times: &[Instant]) {
    let num = ctx.opt.num - ctx.opt.warmup;
    assert!(num > 0);
    let mut delta = Vec::new();
    for i in 0..num {
        delta.push(times[i + ctx.opt.warmup + 1].duration_since(times[i + ctx.opt.warmup]));
    }
    delta.sort_unstable();

    let factor = if ctx.opt.verb == Verb::Read { 1 } else { 2 };

    let cnt = num - LAT_MEASURE_TAIL;
    let mut duration = Duration::new(0, 0);
    let mut lat = Vec::new();
    for t in delta.into_iter().take(cnt) {
        duration += t / factor;
        lat.push(t / factor);
    }
    println!(
        "duration: {:?}, #iters: {}, avg: {:?}, min: {:?}, median: {:?}, P95: {:?}, P99: {:?}, max: {:?}",
        duration,
        cnt,
        duration / cnt as u32,
        lat[0],
        lat[cnt / 2],
        lat[(cnt as f64 * 0.95) as usize],
        lat[(cnt as f64 * 0.99) as usize],
        lat[cnt - 1]
    );
}

pub fn print_bw(ctx: &Context, tposted: &[Instant], tcompleted: &[Instant]) {
    let dura = tcompleted[ctx.opt.num - 1].duration_since(tposted[0]);
    let bw_gbps = (ctx.opt.size * ctx.opt.num) as f64 * 8.0 / dura.as_secs_f64() / 1e9;
    let msg_rate = ctx.opt.num as f64 / dura.as_secs_f64() / 1e6;
    println!(
        "duration: {:?}, bandwidth: {:.2} Gb/s, rate: {:.5} Mpps",
        dura, bw_gbps, msg_rate,
    );
}
