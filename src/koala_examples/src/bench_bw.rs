use libkoala::Error;
use structopt::StructOpt;

use koala_examples::bench::util::{Context, Opts, Test, Verb};

use koala_examples::bench::read_bw;
use koala_examples::bench::send_bw;
use koala_examples::bench::write_bw;

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
    #[structopt(short = "n", long = "num", default_value = "5000")]
    pub num: usize,

    /// Number of warmup iterations.
    #[structopt(short = "w", long = "warmup", default_value = "100")]
    pub warmup: usize,

    /// Message size.
    #[structopt(short = "s", long = "size", default_value = "65536")]
    pub size: usize,
}

fn main() -> Result<(), Error> {
    let args = Args::from_args();
    let ctx = Context::new(
        Opts {
            verb: args.verb,
            ip: args.ip,
            port: args.port,
            num: args.num,
            warmup: args.warmup,
            size: args.size,
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
