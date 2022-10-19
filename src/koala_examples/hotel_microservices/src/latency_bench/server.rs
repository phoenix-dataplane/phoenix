#![feature(scoped_threads)]

use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::{RRef, WRef};

pub mod hotel_microservices {
    pub mod geo {
        // The string specified here must match the proto package name
        mrpc::include_proto!("geo");
    }
}

use hotel_microservices::geo::geo_server::{Geo, GeoServer};
use hotel_microservices::geo::{Request as GeoRequest, Result as GeoResult};

pub struct GeoService;

#[mrpc::async_trait]
impl Geo for GeoService {
    async fn nearby(&self, _request: RRef<GeoRequest>) -> Result<WRef<GeoResult>, mrpc::Status> {
        let mut points = Vec::with_capacity(5);
        for i in 0..5 {
            points.push(i.to_string().into())
        }
        let result = GeoResult { hotel_ids: points };
        let wref = WRef::new(result);
        Ok(wref)
    }
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Hotel microservices latency bench server")]
pub struct Args {
    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    #[structopt(long, default_value = "8")]
    pub reply_size: usize,

    #[structopt(long, default_value = "128")]
    pub provision_count: usize,

    /// Number of server threads.
    #[structopt(long, default_value = "1")]
    pub num_server_threads: usize,
}

fn run_server(tid: usize, args: Args) -> Result<(), mrpc::Error> {
    smol::block_on(async {
        mrpc::stub::LocalServer::bind(format!("0.0.0.0:{}", args.port + tid as u16))?
            .add_service(GeoServer::new(GeoService))
            .serve()
            .await
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        let args = Args::from_args();
        eprintln!("args: {:?}", args);

        for tid in 1..args.num_server_threads {
            let args = args.clone();
            handles.push(s.spawn(move || run_server(tid, args)));
        }

        run_server(0, args)?;
        Ok(())
    })
}
