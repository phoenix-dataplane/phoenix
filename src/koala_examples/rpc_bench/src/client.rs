use std::path::PathBuf;
use std::time::Instant;

use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::stub::RpcMessage;

pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}
use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;

#[derive(StructOpt, Debug)]
#[structopt(about = "Koala RPC hello client")]
pub struct Args {
    /// The address to connect, can be an IP address or domain name.
    #[structopt(short = "c", long = "connect", default_value = "192.168.211.66")]
    pub ip: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// Blocking or not?
    #[structopt(short = "b", long)]
    pub blocking: bool,

    #[structopt(short = "l", long, default_value = "error")]
    pub log_level: String,

    #[structopt(long)]
    pub log_dir: Option<PathBuf>,

    #[structopt(short, long, default_value = "1000000")]
    pub req_size: usize,

    #[structopt(long, default_value = "32")]
    pub concurrency: usize,

    #[structopt(short, long, default_value = "16384")]
    pub total_iters: usize,

    #[structopt(long, default_value = "128")]
    pub provision_count: usize,
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let args = Args::from_args();
    let _guard = init_tokio_tracing(&args.log_level, &args.log_dir);

    let client = GreeterClient::connect((args.ip, args.port))?;
    eprintln!("connection setup");

    if args.blocking {
        smol::block_on(async {
            let mut name = Vec::with_capacity(1000000);
            name.resize(1000000, 42);
            let mut req = RpcMessage::new(HelloRequest { name });

            for i in 0..16384 {
                let _resp = client.say_hello(&mut req).await.unwrap();
                eprintln!("resp {} received", i);
            }

            let start = Instant::now();
            for _i in 0..16384 {
                let _resp = client.say_hello(&mut req).await.unwrap();
            }

            let dura = start.elapsed();
            eprintln!(
                "dura: {:?}, speed: {:?}",
                dura,
                8e-9 * 16384.0 * 1e6 / dura.as_secs_f64()
            );
        });
    } else {
        smol::block_on(async {
            // provision
            let mut reqs = Vec::new();
            for _ in 0..args.provision_count {
                let mut name = Vec::with_capacity(args.req_size);
                name.resize(args.req_size, 42);
                let req = RpcMessage::new(HelloRequest { name });
                reqs.push(req);
            }

            let mut reply_futures = FuturesUnordered::new();

            let mut response_count = 0;

            // warmup
            for i in 0..args.total_iters {
                // let resp = client.say_hello(&mut req).await.unwrap();
                let resp = client.say_hello(&reqs[i % args.provision_count]);
                if reply_futures.len() >= args.concurrency {
                    let _response: Result<_, _> = reply_futures.next().await.unwrap();
                    eprintln!("warmup: resp {} received", response_count);
                    response_count += 1;
                }
                reply_futures.push(resp);
            }

            // warmup: receive the remaining messages
            while !reply_futures.is_empty() {
                let _response: Result<_, _> = reply_futures.next().await.unwrap();
                eprintln!("warmup: resp {} received", response_count);
                response_count += 1;
            }

            let mut starts = Vec::with_capacity(args.total_iters);
            let mut latencies = Vec::with_capacity(args.total_iters);
            let start = Instant::now();
            for i in 0..args.total_iters {
                starts.push(Instant::now());
                let resp = client.say_hello(&reqs[i % args.provision_count]);
                if reply_futures.len() >= args.concurrency {
                    let _response: Result<_, _> = reply_futures.next().await.unwrap();
                    latencies.push(starts[latencies.len()].elapsed());
                }
                reply_futures.push(resp);
            }
            while !reply_futures.is_empty() {
                let _response: Result<_, _> = reply_futures.next().await.unwrap();
                latencies.push(starts[latencies.len()].elapsed());
            }

            let dura = start.elapsed();
            println!(
                "dura: {:?}, speed: {:?}",
                dura,
                8e-9 * args.total_iters as f64 * args.req_size as f64 / dura.as_secs_f64()
            );

            // print latencies
            latencies.sort();
            let cnt = latencies.len();
            println!(
                "duration: {:?}, avg: {:?}, min: {:?}, median: {:?}, p95: {:?}, p99: {:?}, max: {:?}",
                dura,
                dura / starts.len() as u32,
                latencies[0],
                latencies[cnt / 2],
                latencies[(cnt as f64 * 0.95) as usize],
                latencies[(cnt as f64 * 0.99) as usize],
                latencies[cnt - 1]
            );
        });
    }
    Ok(())
}

fn init_tokio_tracing(
    level: &str,
    log_directory: &Option<PathBuf>,
) -> tracing_appender::non_blocking::WorkerGuard {
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    let env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .parse(level)
        .expect("invalid tracing level");

    let (non_blocking, appender_guard) = if let Some(log_dir) = log_directory {
        let file_appender = tracing_appender::rolling::minutely(log_dir, "rpc-client.log");
        tracing_appender::non_blocking(file_appender)
    } else {
        tracing_appender::non_blocking(std::io::stdout())
    };

    tracing_subscriber::fmt::fmt()
        .event_format(format)
        .with_writer(non_blocking)
        .with_env_filter(env_filter)
        .init();

    tracing::info!("tokio_tracing initialized");

    appender_guard
}
