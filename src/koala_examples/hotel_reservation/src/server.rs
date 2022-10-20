use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::{RRef, WRef};

pub mod reservation {
    // The string specified here must match the proto package name
    mrpc::include_proto!("reservation");
}
use reservation::reservation_server::{Reservation, ReservationServer};
use reservation::{Request, Result as ReservationResult};

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Hotel reservation server")]
pub struct Args {
    /// The port number to listen on.
    #[structopt(short, long, default_value = "5050")]
    pub port: u16,
}

#[derive(Debug)]
struct Registry {
    replies: Vec<WRef<ReservationResult>>,
}

#[mrpc::async_trait]
impl Reservation for Registry {
    async fn make_reservation(
        &self,
        _request: RRef<Request>,
    ) -> Result<WRef<ReservationResult>, mrpc::Status> {
        // eprintln!("reply: {:?}", reply);
        Ok(WRef::clone(&self.replies[0]))
    }

    async fn check_availability(
        &self,
        _request: RRef<Request>,
    ) -> Result<WRef<ReservationResult>, mrpc::Status> {
        // eprintln!("reply: {:?}", reply);
        Ok(WRef::clone(&self.replies[0]))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::from_args();
    eprintln!("args: {:?}", args);

    smol::block_on(async {
        // provisioned replies
        let hotel_id = vec!["42".to_string().into()].into();
        let replies = vec![WRef::new(ReservationResult { hotel_id: hotel_id })].into();

        mrpc::stub::LocalServer::bind(format!("0.0.0.0:{}", args.port))?
            .add_service(ReservationServer::new(Registry { replies }))
            .serve()
            .await?;
        Ok(())
    })
}
