use structopt::StructOpt;

use libkoala::Error;

mod bench;
use bench::util::{Context, OptCommand, Opts};
// use bench::read_lat;
use bench::send_lat;
use bench::write_lat;

fn main() -> Result<(), Error> {
    let ctx = Context::new(Opts::from_args());
    ctx.print();

    match ctx.opt.cmd {
        OptCommand::Send => {
            if ctx.client {
                send_lat::run_client(&ctx)?;
            } else {
                send_lat::run_server(&ctx)?;
            }
        }
        OptCommand::Read => {
            if ctx.client {
                // run_read_lat_client();
            } else {
                // run_read_lat_server();
            }
        }
        OptCommand::Write => {
            if ctx.client {
                write_lat::run_client(&ctx)?;
            } else {
                write_lat::run_server(&ctx)?;
            }
        }
    };

    Ok(())
}
