use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use structopt::StructOpt;

#[path = "../config.rs"]
pub mod config;
#[path = "../logging.rs"]
pub mod logging;
pub mod server;
#[path = "../tracer.rs"]
pub mod tracer;

use config::Config;
use server::hotel_microservices::geo::geo_client::GeoClient;
use server::hotel_microservices::rate::rate_client::RateClient;
use server::hotel_microservices::search::search_server::SearchServer;
use server::SearchService;

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Hotel microservices search server")]
pub struct Args {
    /// The port number to listen on.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,
    #[structopt(long, default_value = "geo")]
    pub geo_addr: String,
    #[structopt(long, default_value = "5000")]
    pub geo_port: u16,
    #[structopt(long, default_value = "rate")]
    pub rate_addr: String,
    #[structopt(long, default_value = "5000")]
    pub rate_port: u16,
    #[structopt(short, long)]
    pub config: Option<PathBuf>,
    #[structopt(long)]
    pub log_path: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::from_args();
    if let Some(path) = &args.config {
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let config: Config = serde_json::from_reader(reader)?;
        args.port = config.search_port;
        args.geo_addr = config.geo_addr;
        args.geo_port = config.geo_port;
        args.rate_addr = config.rate_addr;
        args.rate_port = config.rate_port;
        args.log_path = Some(config.log_path.join("search.csv"));
    }
    eprintln!("args: {:?}", args);
    logging::init_env_log("RUST_LOG", "info");

    log::info!("Connecting to geo server...");
    let geo_client = GeoClient::connect(format!("{}:{}", args.geo_addr, args.geo_port))?;
    log::info!("Connecting to rate server...");
    let rate_client = RateClient::connect(format!("{}:{}", args.rate_addr, args.rate_port))?;

    let service = SearchService::new(geo_client, rate_client, args.log_path);
    let signal = async_ctrlc::CtrlC::new()?;
    mrpc::stub::Server::bind(format!("0.0.0.0:{}", args.port))?
        .add_service(SearchServer::new(service))
        .serve_with_graceful_shutdown(signal)
        .await?;
    Ok(())
}
