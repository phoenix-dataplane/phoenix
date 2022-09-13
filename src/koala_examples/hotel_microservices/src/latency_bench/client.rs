#![feature(scoped_threads)]

use std::time::Duration;
use std::task::Poll;

use futures::poll;
use futures::select;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use structopt::StructOpt;
use minstant::Instant;
use hdrhistogram::Histogram;

use mrpc::WRef;

pub mod hotel_microservices {
    pub mod geo {
        // The string specified here must match the proto package name
        mrpc::include_proto!("geo");
    }
}

use hotel_microservices::geo::geo_client::GeoClient;
use hotel_microservices::geo::Request as GeoRequest;

#[derive(StructOpt, Debug)]
#[structopt(about = "Hotel microservices latency bench client")]
pub struct Args {
    /// The address to connect, can be an IP address or domain name.
    #[structopt(short = "c", long = "connect", default_value = "rdma0.danyang-04")]
    pub ip: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5000")]
    pub port: u16,

    /// The maximal number of concurrenty outstanding requests.
    #[structopt(long, default_value = "1")]
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

    /// Whether use a synchronized client that busy polls responses
    #[structopt(long)]
    pub polling: bool,

    /// Interval to send requests (in microsedonds) when polling is used.
    #[structopt(long, default_value = "0")]
    pub send_interval: u64,
}

async fn run_bench_poll(
    args: &Args,
    client: &GeoClient,
    reqs: &[WRef<GeoRequest>],
    _tid: usize,
) -> Result<(Duration, usize, Histogram<u64>), mrpc::Status> {
    eprintln!("busy polling replies");

    let mut hist = hdrhistogram::Histogram::<u64>::new_with_max(60_000_000_000, 5).unwrap();
    assert_eq!(args.concurrency, 1, "concurrrency > 1 is not allowed for polling");

    let (total_iters, timeout) = if let Some(dura) = args.duration {
        (usize::MAX / 2, Duration::from_secs_f64(dura))
    } else {
        (args.total_iters, Duration::from_millis(u64::MAX))
    };

    // start sending
    let start = Instant::now();


    let mut scnt = 0;
    let mut rcnt = 0;

    while scnt < args.warmup {
        let req = WRef::clone(&reqs[0]);
        let mut fut = client.nearby(req);
        let _resp = loop {
            let result = poll!(&mut fut);
            match result {
                Poll::Ready(resp) => break resp,
                Poll::Pending => {}
            }
        }?;
        scnt += 1;
        rcnt += 1;
    }
    let warmup_end = Instant::now();

    while scnt < args.warmup + total_iters {
        std::thread::sleep(Duration::from_micros(args.send_interval));
        let send = Instant::now();
        let req = WRef::clone(&reqs[0]);
        let mut fut = client.nearby(req);
        let _resp = loop {
            let result = poll!(&mut fut);
            match result {
                Poll::Ready(resp) => break resp,
                Poll::Pending => {}
            }
        }?;
        let dura = send.elapsed();
        let _ = hist.record(dura.as_nanos() as u64);
        if start.elapsed() > timeout {
            break;
        }
        scnt += 1;
        rcnt += 1;
    }

    let dura = warmup_end.elapsed();
    Ok((dura, rcnt, hist))
}


async fn run_bench(
    args: &Args,
    client: &GeoClient,
    reqs: &[WRef<GeoRequest>],
    tid: usize,
) -> Result<(Duration, usize, Histogram<u64>), mrpc::Status> {
    let mut hist = hdrhistogram::Histogram::<u64>::new_with_max(60_000_000_000, 5).unwrap();

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

    let mut scnt = 0;
    let mut rcnt = 0;
    let mut last_rcnt = 0;

    while scnt < args.concurrency && scnt < total_iters + args.warmup {
        let slot = scnt;
        starts[slot] = Instant::now();
        let mut req = WRef::clone(&reqs[scnt % args.provision_count]);
        req.set_token(mrpc::Token(slot));
        let fut = client.nearby(req);
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
                }

                rcnt += 1;
                if rcnt == args.warmup {
                    warmup_end = Instant::now();
                }

                if scnt < total_iters + args.warmup {
                    starts[slot] = Instant::now();
                    let mut req = WRef::clone(&reqs[scnt % args.provision_count]);
                    req.set_token(mrpc::Token(slot));
                    let fut = client.nearby(req);
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
                    eprintln!("Thread {}, {} rps", tid, rps);
                    last_ts = Instant::now();
                    last_rcnt = rcnt;
                }
            }
        }
    }

    let dura = warmup_end.elapsed();
    Ok((dura, rcnt, hist))
}

fn run_client_thread(
    tid: usize,
    args: &Args,
) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let client = GeoClient::connect((
        args.ip.as_str(),
        args.port + (tid % args.num_server_threads) as u16,
    ))?;
    eprintln!("connection setup for thread {tid}");

    smol::block_on(async {
        // provision
        let mut reqs = Vec::new();
        for i in 0..args.provision_count {
            let point = GeoRequest {
                lat: 37.78,
                lon: -122.40,
            };
            let req = WRef::with_token(mrpc::Token(i), point);
            reqs.push(req);
        }

        let (dura, rcnt, hist) = if args.polling {
            run_bench_poll(&args, &client, &reqs, tid).await?
        } else {
            run_bench(&args, &client, &reqs, tid).await?
        };

        eprintln!(
            "Thread {tid}, duration: {:?}, rate: {:.5} Mrps",
            dura,
            1e-6 * (rcnt - args.warmup) as f64 / dura.as_secs_f64(),
        );
        // print latencies
        eprintln!(
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