use libkoala::Error;
use structopt::StructOpt;

use bench::util::{Context, Opts, Test, Verb};

use bench::read_bw;
use bench::send_bw;
use bench::write_bw;

#[derive(StructOpt, Debug)]
#[structopt(about = "Koala send/read/write bandwidth.")]
pub struct Args {
    /// Allowed verbs: send, read, write
    #[structopt(name = "verb", parse(from_str), default_value = "send")]
    pub verb: Verb,

    /// The address to connect, can be an IP address or domain name.
    #[structopt(short = "c", long = "connect", default_value = "0.0.0.0")]
    pub ip: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "5000")]
    pub num: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "100")]
    pub warmup: usize,

    /// Message size.
    #[structopt(short, long, default_value = "65536")]
    pub size: usize,

    /// Number of QPs in each thread.
    #[structopt(long, default_value = "1")]
    pub num_qp: usize,

    /// Number of client threads. Map num_client_threads to num_server_threads
    #[structopt(long, default_value = "1")]
    pub num_client_threads: usize,

    /// Number of server threads.
    #[structopt(long, default_value = "1")]
    pub num_server_threads: usize,
}

fn main() -> Result<(), Error> {
    let args = Args::from_args();

    assert!(args.num_client_threads % args.num_server_threads == 0);

    let ctx = Context::new(
        Opts {
            verb: args.verb,
            ip: args.ip,
            port: args.port,
            num: args.num,
            warmup: args.warmup,
            size: args.size,
            num_qp: args.num_qp,
            num_client_threads: args.num_client_threads,
            num_server_threads: args.num_server_threads,
        },
        Test::BW,
    );
    ctx.print();

    match ctx.opt.verb {
        Verb::Send => {
            if ctx.client {
                send_bw::run_client(&ctx)?;
            } else {
                send_bw::run_server(&ctx)?;
            }
        }
        Verb::Read => {
            if ctx.client {
                read_bw::run_client(&ctx)?;
            } else {
                read_bw::run_server(&ctx)?;
            }
        }
        Verb::Write => {
            if ctx.client {
                write_bw::run_client(&ctx)?;
            } else {
                write_bw::run_server(&ctx)?;
            }
        }
    };
    Ok(())
}
