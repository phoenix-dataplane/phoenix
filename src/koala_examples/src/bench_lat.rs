use libkoala::Error;
use structopt::StructOpt;

use koala_examples::bench::util::{Context, Opts, Test, Verb};

use koala_examples::bench::read_lat;
use koala_examples::bench::send_lat;
use koala_examples::bench::write_lat;

fn main() -> Result<(), Error> {
    let ctx = Context::new(Opts::from_args(), Test::LAT);
    ctx.print();

    match ctx.opt.verb {
        Verb::Send => {
            if ctx.client {
                send_lat::run_client(&ctx)?;
            } else {
                send_lat::run_server(&ctx)?;
            }
        }
        Verb::Read => {
            if ctx.client {
                read_lat::run_client(&ctx)?;
            } else {
                read_lat::run_server(&ctx)?;
            }
        }
        Verb::Write => {
            if ctx.client {
                write_lat::run_client(&ctx)?;
            } else {
                write_lat::run_server(&ctx)?;
            }
        }
    };

    Ok(())
}
