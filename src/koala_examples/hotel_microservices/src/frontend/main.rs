use std::convert::Infallible;
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use structopt::StructOpt;

#[path = "../config.rs"]
pub mod config;
#[path = "../logging.rs"]
pub mod logging;
pub mod server;
#[path = "../tracer.rs"]
pub mod tracer;

use config::Config;
use server::hotel_microservices::profile::profile_client::ProfileClient;
use server::hotel_microservices::search::search_client::SearchClient;
use server::{dispatch_fn, FrontendService};

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Hotel microservices frontend server")]
pub struct Args {
    /// The port number to listen on.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,
    #[structopt(long, default_value = "search")]
    pub search_addr: String,
    #[structopt(long, default_value = "5000")]
    pub search_port: u16,
    #[structopt(long, default_value = "profile")]
    pub profile_addr: String,
    #[structopt(long, default_value = "5000")]
    pub profile_port: u16,
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
        args.port = config.frontend_port;
        args.search_addr = config.search_addr;
        args.search_port = config.search_port;
        args.profile_addr = config.profile_addr;
        args.profile_port = config.profile_port;
        args.log_path = Some(config.log_path.join("frontend.csv"));
    }
    eprintln!("args: {:?}", args);
    logging::init_env_log("RUST_LOG", "info");

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let search_client =
        SearchClient::connect(format!("{}:{}", args.search_addr, args.search_port))?;
    let profile_client =
        ProfileClient::connect(format!("{}:{}", args.profile_addr, args.profile_port))?;
    let frontend = Arc::new(FrontendService::new(
        search_client,
        profile_client,
        args.log_path,
    ));

    let make_service = make_service_fn(move |_conn| {
        let frontend = frontend.clone();
        let service = service_fn(move |req| dispatch_fn(frontend.clone(), req));

        async move { Ok::<_, Infallible>(service) }
    });

    let server = Server::bind(&addr).serve(make_service);

    let signal = async_ctrlc::CtrlC::new()?;
    let graceful = server.with_graceful_shutdown(signal);
    if let Err(e) = graceful.await {
        log::error!("Server error: {}", e);
    }
    Ok(())
}
