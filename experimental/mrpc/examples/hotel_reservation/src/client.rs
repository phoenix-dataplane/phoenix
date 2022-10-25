use std::future::Future;
use std::time::Duration;

use futures::select;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use hdrhistogram::Histogram;
use minstant::Instant;
use prost::Message;
use structopt::StructOpt;

use mrpc::WRef;

pub mod reservation {
    // The string specified here must match the proto package name
    mrpc::include_proto!("reservation");
}
use reservation::reservation_client::ReservationClient;
use reservation::Request;

#[derive(StructOpt, Debug)]
#[structopt(about = "Hotel reservation client")]
pub struct Args {
    /// The address to connect, can be an IP address or domain name.
    #[structopt(short = "c", long = "connect", default_value = "192.168.211.66")]
    pub server_addr: String,

    /// The port number to use.
    #[structopt(short, long, default_value = "5050")]
    pub port: u16,

    /// The maximal number of concurrenty outstanding requests.
    #[structopt(long, default_value = "32")]
    pub concurrency: usize,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "16384")]
    pub total_iters: usize,

    /// Run test for a customized period of seconds.
    #[structopt(short = "D", long)]
    pub duration: Option<f64>,

    /// Seconds between periodic throughput reports.
    #[structopt(short, long)]
    pub interval: Option<f64>,
}

#[derive(Debug)]
struct Call {
    ts: Instant,
    req_size: usize,
    result: Result<mrpc::RRef<reservation::Result>, mrpc::Status>,
}

fn make_reservation<'c>(
    client: &'c ReservationClient,
    workload: &Workload,
) -> impl Future<Output = Call> + 'c {
    let ts = Instant::now();
    let (req, req_size) = workload.next_request();
    let fut = client.make_reservation(req);
    async move {
        Call {
            ts,
            req_size,
            result: fut.await,
        }
    }
}

async fn run_bench(
    args: &Args,
    client: &ReservationClient,
    workload: &Workload,
) -> Result<(Duration, usize, usize, Histogram<u64>), mrpc::Status> {
    let mut hist = hdrhistogram::Histogram::<u64>::new_with_max(60_000_000_000, 5).unwrap();

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

    let mut nbytes = 0;
    let mut last_nbytes = 0;

    let mut scnt = 0;
    let mut rcnt = 0;
    let mut last_rcnt = 0;

    while scnt < args.concurrency && scnt < total_iters {
        let fut = make_reservation(client, workload);
        reply_futures.push(fut);
        scnt += 1;
    }

    let mut blocked = 0;

    loop {
        select! {
            resp = reply_futures.next() => {
                if rcnt >= total_iters || start.elapsed() > timeout {
                    break;
                }

                let Call { ts, req_size, result: resp }  = resp.unwrap();
                if let Err(_status) = resp {
                    blocked += 1;
                }

                let dura = ts.elapsed();
                let _ = hist.record(dura.as_nanos() as u64);
                nbytes += req_size;

                rcnt += 1;

                if scnt < total_iters {
                    let fut = make_reservation(client, workload);
                    reply_futures.push(fut);
                    scnt += 1;
                }
            }
            complete => break,
            default => {
                // no futures is ready
                if rcnt >= total_iters || start.elapsed() > timeout {
                    break;
                }
                let last_dura = last_ts.elapsed();
                if tput_interval.is_some() && last_dura > tput_interval.unwrap() {
                    let rps = (rcnt - last_rcnt) as f64 / last_dura.as_secs_f64();
                    let bw = 8e-9 * (nbytes - last_nbytes) as f64 / last_dura.as_secs_f64();
                    println!("{} rps, {} Gb/s", rps, bw);
                    last_ts = Instant::now();
                    last_rcnt = rcnt;
                    last_nbytes = nbytes;
                }
            }
        }
    }

    let dura = start.elapsed();
    println!("blocked calls: {}", blocked);
    Ok((dura, nbytes, rcnt, hist))
}

// auto GenerateWorkload() -> std::tuple<Request&, size_t> {
//   static bool init = false;
//   static Request request[2];
//   static size_t req_size[2];
//   if (!init) {
//     request[0].set_customername("danyang");
//     request[0].add_hotelid();
//     request[0].set_hotelid(0, kHotelId);
//     request[0].set_indate("2022-09-01");
//     request[0].set_outdate("2022-09-02");
//     request[0].set_roomnumber(128);
//
//     request[1].set_customername("matt");
//     request[1].add_hotelid();
//     request[1].set_hotelid(0, kHotelId);
//     request[1].set_indate("2022-09-02");
//     request[1].set_outdate("2022-09-03");
//     request[1].set_roomnumber(129);
//
//     req_size[0] = request[0].ByteSizeLong();
//     req_size[1] = request[1].ByteSizeLong();
//
//     init = true;
//   }
//   // 1% danyang, 99% matt
//   int index = 1 - (int)(fast_rand() % 100 < 1);
//   return {request[index], req_size[index]};
// }

struct Workload {
    reqs: Vec<WRef<Request>>,
    req_sizes: Vec<usize>,
}

impl Workload {
    fn new() -> Self {
        let req1 = Request {
            customer_name: "danyang".into(),
            hotel_id: vec!["42".into()].into(),
            in_date: "2022-09-01".into(),
            out_date: "2022-09-02".into(),
            room_number: 128,
        };
        let req2 = Request {
            customer_name: "matt".into(),
            hotel_id: vec!["42".into()].into(),
            in_date: "2022-09-02".into(),
            out_date: "2022-09-03".into(),
            room_number: 129,
        };
        let req_sizes = vec![req1.encoded_len(), req2.encoded_len()];
        Self {
            reqs: vec![WRef::new(req1), WRef::new(req2)],
            req_sizes,
        }
    }

    fn next_request(&self) -> (WRef<Request>, usize) {
        let index = (fastrand::usize(..100) >= 1) as usize;
        (WRef::clone(&self.reqs[index]), self.req_sizes[index])
    }
}

fn run_client(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let client = ReservationClient::connect((args.server_addr.as_str(), args.port))?;

    smol::block_on(async {
        // provision
        let workload = Workload::new();

        let (dura, total_bytes, rcnt, hist) = run_bench(args, &client, &workload).await?;

        println!(
            "duration: {:?}, bandwidth: {:?} Gb/s, rate: {:.5} Mrps",
            dura,
            8e-9 * total_bytes as f64 / dura.as_secs_f64(),
            1e-6 * rcnt as f64 / dura.as_secs_f64(),
        );
        // print latencies
        println!(
            "duration: {:?}, avg: {:?}, min: {:?}, median: {:?}, p95: {:?}, p99: {:?}, max: {:?}",
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

    run_client(&args).unwrap();

    Ok(())
}
