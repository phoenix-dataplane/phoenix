use std::convert::From;
use std::time::Instant;
use structopt::StructOpt;

use interface::{addrinfo, WcStatus, WorkCompletion};
use libkoala::{cm, verbs, Error};

#[derive(StructOpt, Debug, PartialEq)]
pub enum OptCommand {
    Send,
    Read,
    Write,
}

impl From<&str> for OptCommand {
    fn from(cmd: &str) -> Self {
        match cmd.to_lowercase().as_str() {
            "read" => OptCommand::Read,
            "write" => OptCommand::Write,
            _ => OptCommand::Send,
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(about = "Koala send/read/write latency.")]
pub struct Opts {
    /// Allowed operations: send, read, write
    #[structopt(name = "operation", parse(from_str), default_value = "send")]
    pub cmd: OptCommand,

    /// The address to connect, can be an IP address or domain name.
    #[structopt(short = "c", long = "connect", default_value = "0.0.0.0")]
    pub ip: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Total number of iterations.
    #[structopt(short = "n", long = "num", default_value = "1000")]
    pub num: usize,

    /// Number of warmup iterations.
    #[structopt(short = "w", long = "warm", default_value = "100")]
    pub warm: usize,

    /// Message size.
    #[structopt(short = "s", long = "size", default_value = "8")]
    pub size: usize,
}

pub struct Context<'ctx> {
    pub opt: Opts,
    pub client: bool,
    pub ai: addrinfo::AddrInfo,
    pub attr: verbs::QpInitAttr<'ctx>,
}

impl<'ctx> Context<'ctx> {
    pub fn new(opt: Opts) -> Self {
        let ai =
            cm::getaddrinfo(Some(&opt.ip), Some(&opt.port.to_string()), None).expect("getaddrinfo");

        let attr = verbs::QpInitAttr {
            qp_context: None,
            send_cq: None,
            recv_cq: None,
            srq: None,
            cap: verbs::QpCapability {
                max_send_wr: 1024,
                max_recv_wr: 1024,
                max_send_sge: 1,
                max_recv_sge: 1,
                max_inline_data: 236,
            },
            qp_type: verbs::QpType::RC,
            sq_sig_all: false,
        };

        Context {
            client: opt.ip != "0.0.0.0",
            opt: opt,
            ai: ai,
            attr: attr,
        }
    }

    pub fn print(self: &Self) {
        println!("machine: {}", if self.client { "client" } else { "sever" });
        println!(
            "num:{}, size:{}, warmup:{}",
            self.opt.num, self.opt.size, self.opt.warm
        );
        match self.opt.cmd {
            OptCommand::Send => println!("Send data from client to server"),
            OptCommand::Read => println!("Read data from server to client"),
            OptCommand::Write => println!("Write data from client to server"),
        }
    }
}

pub fn poll_cq_and_check(
    cq: &verbs::CompletionQueue,
    wcs: &mut Vec<WorkCompletion>,
) -> Result<(), Error> {
    loop {
        cq.poll_cq(wcs)?;
        if !wcs.is_empty() {
            break;
        }
    }
    for wc in wcs {
        if wc.status != WcStatus::Success {
            panic!("wc status is not succcss: {}", wc.vendor_err);
        }
    }
    Ok(())
}

const LAT_MEASURE_TAIL: usize = 2;
pub fn print_lat(ctx: &Context, times: Vec<Instant>) {
    let num = ctx.opt.num - ctx.opt.warm;
    assert!(num > 0);
    let mut delta = Vec::new();
    for i in 0..num {
        delta.push(
            times[i + ctx.opt.warm + 1]
                .duration_since(times[i + ctx.opt.warm])
                .as_micros(),
        );
    }
    delta.sort();

    let factor = if ctx.opt.cmd == OptCommand::Read {
        1.0
    } else {
        2.0
    };

    let cnt = num - LAT_MEASURE_TAIL;
    let mut duration = 0.0;
    let mut lat = Vec::new();
    for i in 0..cnt {
        let t = delta[i] as f64 / factor;
        duration += t;
        lat.push(t);
    }
    println!(
        "duration: {}, avg: {}, min: {}, median: {}, p95: {}, p99: {}, max: {}",
        duration,
        duration / cnt as f64,
        lat[0],
        lat[cnt / 2],
        lat[(cnt as f64 * 0.95) as usize],
        lat[(cnt as f64 * 0.99) as usize],
        lat[cnt - 1]
    );
}
