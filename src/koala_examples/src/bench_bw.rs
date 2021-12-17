use libkoala::Error;
use structopt::StructOpt;

use koala_examples::bench::util::{Context, Opts, Test, Verb};

use koala_examples::bench::read_bw;
use koala_examples::bench::send_bw;
use koala_examples::bench::write_bw;

fn main() -> Result<(), Error> {
    let ctx = Context::new(Opts::from_args(), Test::BW);
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
