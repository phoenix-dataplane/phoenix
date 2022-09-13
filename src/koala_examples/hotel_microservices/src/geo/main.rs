use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use structopt::StructOpt;

#[path = "../config.rs"]
pub mod config;
pub mod db;
#[path = "../logging.rs"]
pub mod logging;
pub mod server;
#[path = "../tracer.rs"]
pub mod tracer;

use config::Config;
use db::initialize_database;
use server::hotel_microservices::geo::geo_server::GeoServer;
use server::GeoService;

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Hotel reservation geo server")]
pub struct Args {
    /// The port number to listen on.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,
    #[structopt(long, default_value = "mongodb://localhost:27017")]
    pub db: String,
    #[structopt(short, long)]
    pub config: Option<PathBuf>,
    #[structopt(long)]
    pub log_path: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::from_args();
    if let Some(path) = &args.config {
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let config: Config = serde_json::from_reader(reader)?;
        args.db = config.geo_mongo_addr;
        args.port = config.geo_port;
        args.log_path = Some(config.log_path.join("geo.csv"));
    }
    eprintln!("args: {:?}", args);
    logging::init_env_log("RUST_LOG", "info");

    log::info!("Initializing DB connection...");
    let database = initialize_database(args.db)?;
    log::info!("Successful");

    let service = GeoService::new(database, args.log_path)?;
    let signal = async_ctrlc::CtrlC::new()?;
    smol::block_on(async {
        mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
            .add_service(GeoServer::new(service))
            .serve_with_graceful_shutdown(signal)
            .await?;
        Ok(())
    })
}
