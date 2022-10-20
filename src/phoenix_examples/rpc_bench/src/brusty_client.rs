#![feature(scoped_threads)]
use std::path::PathBuf;
use std::time::Duration;

use futures::select;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use hdrhistogram::Histogram;
use minstant::Instant;
use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::WRef;

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

    /// Log level for tracing.
    #[structopt(short = "l", long, default_value = "error")]
    pub log_level: String,

    /// Log directory.
    #[structopt(long)]
    pub log_dir: Option<PathBuf>,

    /// Whether to log latency metrics.
    #[structopt(long)]
    pub log_latency: bool,

    /// Interval to send brush requests (in microsedonds).
    #[structopt(long, default_value = "0")]
    pub send_interval: u64,

    /// Startup delay (in seconds).
    #[structopt(long, default_value = "0")]
    pub startup_delay: u64,

    /// Request size.
    #[structopt(short, long, default_value = "1000000")]
    pub req_size: usize,

    /// The maximal number of concurrenty outstanding requests.
    #[structopt(long, default_value = "32")]
    pub concurrency: usize,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "16384")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "1000")]
    pub warmup: usize,

    /// Run test for a customized period of seconds.
    #[structopt(short = "D", long)]
    pub duration: Option<f64>,

    /// Seconds between periodic throughput reports.
    #[structopt(short, long)]
    pub interval: Option<f64>,

    /// The number of messages to provision. Must be a positive number. The test will repeatedly
    /// sending the provisioned message.
    #[structopt(long, default_value = "1")]
    pub provision_count: usize,

    /// Number of client threads. Each client thread is mapped to one server threads.
    #[structopt(long, default_value = "1")]
    pub num_client_threads: usize,

    /// Number of server threads.
    #[structopt(long, default_value = "1")]
    pub num_server_threads: usize,
}

// mod bench_app;
// include!("./bench_app.rs");

#[allow(unused)]
async fn run_bench(
    args: &Args,
    client: &GreeterClient,
    reqs: &[WRef<HelloRequest>],
    tid: usize,
) -> Result<(Duration, usize, usize, Histogram<u64>), mrpc::Status> {
    macro_rules! my_print {
        ($($arg:tt)*) => {
            if args.log_level == "info" {
                tracing::info!($($arg)*);
            } else {
                println!($($arg)*);
            }
        }
    }

    let mut hist = hdrhistogram::Histogram::<u64>::new_with_max(60_000_000_000, 5).unwrap();

    let mut rpc_size = vec![0; args.concurrency];
    let mut starts = Vec::with_capacity(args.concurrency);
    let now = Instant::now();
    starts.resize(starts.capacity(), now);

    let mut reply_futures = FuturesUnordered::new();

    let (total_iters, timeout) = if let Some(dura) = args.duration {
        (usize::MAX / 2, Duration::from_secs_f64(dura))
    } else {
        (args.total_iters, Duration::from_millis(u64::MAX))
    };

    // report the rps every several milliseconds
    let tput_interval = args.interval.map(Duration::from_secs_f64);

    // start sending
    let mut last_ts = Instant::now();
    let start = Instant::now();

    let mut warmup_end = Instant::now();
    let mut nbytes = 0;
    let mut last_nbytes = 0;

    let mut scnt = 0;
    let mut rcnt = 0;
    let mut last_rcnt = 0;

    while scnt < args.concurrency && scnt < total_iters + args.warmup {
        let slot = scnt;
        starts[slot] = Instant::now();
        let mut req = WRef::clone(&reqs[scnt % args.provision_count]);
        req.set_token(mrpc::Token(slot));
        let fut = client.say_hello(req);
        reply_futures.push(fut);
        scnt += 1;
    }

    loop {
        select! {
            resp = reply_futures.next() => {
                if rcnt >= total_iters + args.warmup || start.elapsed() > timeout {
                    break;
                }

                let resp = resp.unwrap()?;
                let slot = resp.token().0 % args.concurrency;

                if rcnt >= args.warmup {
                    let dura = starts[slot].elapsed();
                    let _ = hist.record(dura.as_nanos() as u64);
                    nbytes += rpc_size[slot];
                }

                rcnt += 1;
                if rcnt == args.warmup {
                    warmup_end = Instant::now();
                }

                if rcnt % args.concurrency == 0 {
                    std::thread::sleep(Duration::from_micros(args.send_interval));
                    let mut round_scnt = 0;
                    while round_scnt < args.concurrency && scnt < total_iters + args.warmup {
                        let slot = round_scnt;
                        starts[slot] = Instant::now();
                        rpc_size[slot] = args.req_size;
                        let mut req = WRef::clone(&reqs[scnt % args.provision_count]);
                        req.set_token(mrpc::Token(slot));
                        let fut = client.say_hello(req);
                        reply_futures.push(fut);
                        scnt += 1;
                        round_scnt += 1;
                    }
                }
            }
            complete => break,
            default => {
                // no futures is ready
                if rcnt >= total_iters + args.warmup || start.elapsed() > timeout {
                    break;
                }
                let last_dura = last_ts.elapsed();
                if tput_interval.is_some() && last_dura > tput_interval.unwrap() {
                    let rps = (rcnt - last_rcnt) as f64 / last_dura.as_secs_f64();
                    let bw = 8e-9 * (nbytes - last_nbytes) as f64 / last_dura.as_secs_f64();
                    if args.log_latency {
                        my_print!(
                            "Thread {}, {} rps, {} Gb/s, p95: {:?}, p99: {:?}",
                            tid,
                            rps,
                            bw,
                            Duration::from_nanos(hist.value_at_percentile(95.0)),
                            Duration::from_nanos(hist.value_at_percentile(99.0)),
                        );
                    } else {
                        my_print!("Thread {}, {} rps, {} Gb/s", tid, rps, bw);
                    }
                    last_ts = Instant::now();
                    last_rcnt = rcnt;
                    last_nbytes = nbytes;
                }
            }
        }
    }

    let dura = warmup_end.elapsed();
    Ok((dura, nbytes, rcnt, hist))
}

fn run_client_thread(
    tid: usize,
    args: &Args,
) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    macro_rules! my_print {
        ($($arg:tt)*) => {
            if args.log_level == "info" {
                tracing::info!($($arg)*);
            } else {
                println!($($arg)*);
            }
        }
    }

    let client = GreeterClient::connect((
        args.ip.as_str(),
        args.port + (tid % args.num_server_threads) as u16,
    ))?;
    eprintln!("connection setup for thread {tid}");

    smol::block_on(async {
        // provision
        let mut reqs = Vec::new();
        for i in 0..args.provision_count {
            let mut name = Vec::with_capacity(args.req_size);
            name.resize(args.req_size, 42);
            let req = WRef::with_token(mrpc::Token(i), HelloRequest { name });
            reqs.push(req);
        }

        std::thread::sleep(Duration::from_secs(args.startup_delay));
        let (dura, total_bytes, rcnt, hist) = run_bench(&args, &client, &reqs, tid).await?;

        my_print!(
            "Thread {tid}, duration: {:?}, bandwidth: {:?} Gb/s, rate: {:.5} Mrps",
            dura,
            8e-9 * total_bytes as f64 / dura.as_secs_f64(),
            1e-6 * (rcnt - args.warmup) as f64 / dura.as_secs_f64(),
        );
        // print latencies
        my_print!(
            "Thread {tid}, duration: {:?}, avg: {:?}, min: {:?}, median: {:?}, p95: {:?}, p99: {:?}, max: {:?}",
            dura,
            Duration::from_nanos(hist.mean() as u64),
            Duration::from_nanos(hist.min()),
            Duration::from_nanos(hist.value_at_percentile(50.0)),
            Duration::from_nanos(hist.value_at_percentile(95.0)),
            Duration::from_nanos(hist.value_at_percentile(99.0)),
            Duration::from_nanos(hist.max()),
        );

        Result::<(), mrpc::Status>::Ok(())
    })?;

    Ok(())
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let args = Args::from_args();
    eprintln!("args: {:?}", args);

    assert!(args.num_client_threads % args.num_server_threads == 0);

    let _guard = init_tokio_tracing(&args.log_level, &args.log_dir);

    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for tid in 1..args.num_client_threads {
            let args = &args;
            handles.push(s.spawn(move || {
                run_client_thread(tid, args).unwrap();
            }));
        }
        run_client_thread(0, &args).unwrap();
    });

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
        .with_ansi(false)
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
