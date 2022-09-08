#![feature(scoped_threads)]

use std::cmp;
use std::path::PathBuf;
use std::time::Duration;

use futures::select;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use hdrhistogram::Histogram;
use minstant::Instant;
use structopt::clap::arg_enum;
use structopt::StructOpt;

use mrpc::alloc::Vec;
use mrpc::WRef;

pub mod rpc_plus {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_plus");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_plus::greeter_client::GreeterClient;
use rpc_plus::HelloRequest;

arg_enum! {
    #[derive(Debug,Clone,Copy)]
    pub enum ModelType{
        MobileNet,
        EfficientNet,
        InceptionV3,
    }
}

#[derive(StructOpt, Debug, Clone)]
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

    /// overwrite with hardcoded test case
    #[structopt(long)]
    pub overwrite: Option<ModelType>,
}

// mod bench_app;
// include!("./bench_app.rs");

#[allow(unused)]
struct TraceData {
    data: Vec<usize>,
    cursor: usize,
}

impl TraceData {
    fn new(model_type: ModelType) -> TraceData {
        let data_ori = match model_type {
            ModelType::MobileNet => vec![
                3456, 512, 1152, 512, 8192, 1024, 2304, 1024, 32768, 2048, 4608, 2048, 65536, 2048,
                4608, 2048, 131072, 4096, 9216, 4096, 262144, 4096, 9216, 4096, 524288, 8192,
                18432, 8192, 1048576, 8192, 18432, 8192, 1048576, 8192, 18432, 8192, 1048576, 8192,
                18432, 8192, 1048576, 8192, 18432, 8192, 1048576, 8192, 18432, 8192, 2097152,
                16384, 36864, 16384, 4194304, 16384, 4100000,
            ],
            ModelType::EfficientNet => vec![
                28, 3456, 512, 1152, 512, 1056, 1152, 2048, 256, 6144, 1536, 3456, 1536, 1552,
                1920, 9216, 384, 13824, 2304, 5184, 2304, 3480, 4032, 13824, 384, 13824, 2304,
                14400, 2304, 3480, 4032, 23040, 640, 38400, 3840, 24000, 3840, 9640, 10560, 38400,
                640, 38400, 3840, 8640, 3840, 9640, 10560, 76800, 1280, 153600, 7680, 17280, 7680,
                38480, 40320, 153600, 1280, 153600, 7680, 17280, 7680, 38480, 40320, 153600, 1280,
                153600, 7680, 48000, 7680, 38480, 40320, 215040, 1792, 301056, 10752, 67200, 10752,
                75376, 77952, 301056, 1792, 301056, 10752, 67200, 10752, 75376, 77952, 301056,
                1792, 301056, 10752, 67200, 10752, 75376, 77952, 516096, 3072, 884736, 18432,
                115200, 18432, 221376, 225792, 884736, 3072, 884736, 18432, 115200, 18432, 221376,
                225792, 884736, 3072, 884736, 18432, 115200, 18432, 221376, 225792, 884736, 3072,
                884736, 18432, 41472, 18432, 221376, 225792, 1474560, 5120, 1638400, 20480,
                5124000,
            ],
            ModelType::InceptionV3 => vec![
                3456, 384, 36864, 384, 73728, 768, 20480, 960, 552960, 2304, 49152, 768, 36864,
                221184, 576, 1152, 49152, 307200, 331776, 24576, 768, 768, 1152, 384, 65536, 768,
                49152, 221184, 576, 1152, 65536, 307200, 331776, 65536, 768, 768, 1152, 768, 73728,
                768, 55296, 221184, 576, 1152, 73728, 307200, 331776, 73728, 768, 768, 1152, 768,
                73728, 768, 221184, 1152, 3981312, 331776, 4608, 1152, 393216, 1536, 458752, 1536,
                393216, 458752, 1536, 1536, 458752, 458752, 1536, 1536, 589824, 688128, 688128,
                589824, 2304, 2304, 2304, 2304, 491520, 1920, 716800, 1920, 491520, 716800, 1920,
                1920, 716800, 716800, 1920, 1920, 589824, 860160, 860160, 589824, 2304, 2304, 2304,
                2304, 491520, 1920, 716800, 1920, 491520, 716800, 1920, 1920, 716800, 716800, 1920,
                1920, 589824, 860160, 860160, 589824, 2304, 2304, 2304, 2304, 589824, 2304,
                1032192, 2304, 589824, 1032192, 2304, 2304, 1032192, 1032192, 2304, 2304, 589824,
                1032192, 1032192, 589824, 2304, 2304, 2304, 2304, 589824, 2304, 1032192, 2304,
                589824, 1032192, 2304, 2304, 2211840, 1327104, 3840, 2304, 2293760, 5376, 1966080,
                6193152, 4608, 4608, 1769472, 1769472, 1769472, 1769472, 1638400, 4608, 4608, 4608,
                4608, 983040, 3840, 2304, 3670016, 5376, 3145728, 6193152, 4608, 4608, 1769472,
                1769472, 1769472, 1769472, 2621440, 4608, 4608, 4608, 4608, 1572864, 3840, 2304,
                8196000,
            ],
        };

        let mut data = Vec::with_capacity(data_ori.len());
        for i in data_ori {
            data.push(i);
        }
        TraceData { data, cursor: 0 }
    }

    fn next(&mut self) -> Option<usize> {
        if self.cursor < self.data.len() {
            Some(self.data[self.cursor])
        } else {
            None
        }
    }

    fn repeat_next(&mut self) -> usize {
        let r = self.data[self.cursor];
        self.cursor = if self.cursor == self.data.len() - 1 {
            0
        } else {
            self.cursor + 1
        };
        r
    }
}

#[allow(unused)]
async fn run_bench(
    args: &Args,
    client: &GreeterClient,
    reqs: &[WRef<HelloRequest>],
    tid: usize,
) -> Result<(Duration, usize, usize, Histogram<u64>), mrpc::Status> {
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

    let mut trace_data = if args.overwrite.is_some() {
        Some(TraceData::new(args.overwrite.unwrap()))
    } else {
        None
    };

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

                if scnt < total_iters + args.warmup {
                    starts[slot] = Instant::now();
                    rpc_size[slot] = if trace_data.is_some(){
                        trace_data.as_mut().unwrap().repeat_next()
                    }else{args.req_size}+12; // todo: NOTICE HERE!
                    let mut req = WRef::clone(&reqs[scnt % args.provision_count]);
                    req.set_token(mrpc::Token(slot));
                    let fut = client.say_hello(req);
                    reply_futures.push(fut);
                    scnt += 1;
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
                    println!("Thread {}, {} rps, {} Gb/s", tid, rps, bw);
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
    args_o: &Args,
) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let client = GreeterClient::connect((
        args_o.ip.as_str(),
        args_o.port + (tid % args_o.num_server_threads) as u16,
    ))?;
    eprintln!("connection setup for thread {tid}");
    let mut args = args_o.clone();

    smol::block_on(async {
        let reqs = if args.overwrite.is_some() {
            // provision
            let mut trace_data = TraceData::new(args.overwrite.unwrap());
            args.total_iters = trace_data.data.len();
            args.duration = None;
            args.provision_count = args.total_iters;
            args.concurrency = cmp::min(args.total_iters, 100);
            args.warmup = (args.warmup / trace_data.data.len() + 1) * trace_data.data.len();
            let mut reqs = Vec::new();
            for i in 0..args.provision_count {
                let mut key = Vec::with_capacity(8);
                key.resize(8, 42);
                let mut len = Vec::with_capacity(4);
                len.resize(4, 42);
                let payload_size = trace_data.next().unwrap();
                let mut payload = Vec::with_capacity(payload_size);
                payload.resize(payload_size, 42);
                let req = WRef::with_token(mrpc::Token(i), HelloRequest { key, payload, len });
                reqs.push(req);
            }
            reqs
        } else {
            // provision
            let mut reqs = Vec::new();
            for i in 0..args.provision_count {
                let mut key = Vec::with_capacity(8);
                key.resize(8, 42);
                let mut len = Vec::with_capacity(4);
                len.resize(4, 42);
                let mut payload = Vec::with_capacity(args.req_size);
                payload.resize(args.req_size, 42);
                let req = WRef::with_token(mrpc::Token(i), HelloRequest { key, payload, len });
                reqs.push(req);
            }
            reqs
        };

        let (dura, total_bytes, rcnt, hist) = run_bench(&args, &client, &reqs, tid).await?;

        println!(
            "Thread {tid}, duration: {:?}, bandwidth: {:?} Gb/s, rate: {:.5} Mrps",
            dura,
            8e-9 * total_bytes as f64 / dura.as_secs_f64(),
            1e-6 * (rcnt - args.warmup) as f64 / dura.as_secs_f64(),
        );
        // print latencies
        println!(
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
