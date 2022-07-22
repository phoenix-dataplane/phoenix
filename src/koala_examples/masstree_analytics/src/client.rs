#![feature(exclusive_range_pattern)]
#![feature(scoped_threads)]
use std::mem;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use crossbeam_utils::CachePadded;
use fasthash::city;
use futures::select;
use futures::stream::{FuturesUnordered, StreamExt};
use minstant::Instant;
use nix::sys::signal;
use structopt::StructOpt;

use mrpc::{Token, WRef};

#[allow(unused)]
mod latency;

mod logging;

// Workload params
const APP_MAX_REQ_WINDOW: usize = 8;
const PRINT_STATS_PERIOD_MS: u64 = 500;

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigint(sig: i32) {
    assert_eq!(sig, signal::SIGINT as i32);
    TERMINATE.store(true, Ordering::Relaxed);
}

#[derive(Debug, StructOpt)]
struct Opt {
    /// Test milliseconds
    #[structopt(long)]
    test_ms: u64,
    /// Number of client threads
    #[structopt(long)]
    num_client_threads: usize,
    /// Outstanding requests per client thread
    #[structopt(long)]
    req_window: usize,
    /// Number of keys in the server's Masstree
    #[structopt(long)]
    num_keys: usize,
    /// Size of range to scan
    #[structopt(long)]
    range_size: usize,
    /// Percentage of range scans
    #[structopt(long)]
    range_req_percent: usize,
    /// Server address, could be an IP address or domain name. If not specified, server will use
    /// 0.0.0.0 and client will use 127.0.0.1
    #[structopt(long)]
    server_addr: Option<String>,
    /// Server port
    #[structopt(long)]
    server_port: u16,
    /// Number of server foreground threads
    #[structopt(long)]
    num_server_fg_threads: usize,
}

pub mod masstree_analytics {
    mrpc::include_proto!("masstree_analytics");
}

use crate::masstree_analytics::masstree_analytics_client::MasstreeAnalyticsClient;
use crate::masstree_analytics::{PointRequest, RangeRequest};

#[derive(Debug)]
enum Query {
    Point(WRef<PointRequest>),
    Range(WRef<RangeRequest>),
}

// The keys in the index are 64-bit hashes of keys {0, ..., num_keys}.
// This gives us random-ish 64-bit keys, without requiring actually maintaining
// the set of inserted keys
fn get_random_key(num_keys: usize) -> u64 {
    let generator_key = fastrand::usize(..num_keys);
    city::hash64(unsafe {
        slice::from_raw_parts(
            &generator_key as *const _ as *const u8,
            mem::size_of_val(&generator_key),
        )
    })
}

// Helper function for clients
fn generate_workload(opt: &Opt) -> Vec<Query> {
    let mut workload = Vec::with_capacity(opt.req_window);
    for i in 0..opt.req_window {
        let key = get_random_key(opt.num_keys);
        if fastrand::usize(..100) < opt.range_req_percent {
            // Generate a range query
            let req = RangeRequest {
                key,
                range: opt.range_size as _,
            };
            workload.push(Query::Range(WRef::with_token(Token(i), req)));
        } else {
            // Generate a point query
            let req = PointRequest { key };
            workload.push(Query::Point(WRef::with_token(Token(i), req)));
        }
    }
    workload
}

#[derive(Debug, Clone)]
struct Client {
    tput_t0: Instant,
    num_resps_tot: usize,
    point_latency: latency::Latency,
    range_latency: latency::Latency,
}

impl Client {
    fn new() -> Self {
        Self {
            tput_t0: Instant::now(),
            num_resps_tot: 0,
            point_latency: latency::Latency::new(),
            range_latency: latency::Latency::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Stats {
    mrps: f64,
    lat_us_50: f64,
    lat_us_99: f64,
}

use std::ops::AddAssign;
impl AddAssign for Stats {
    fn add_assign(&mut self, rhs: Self) {
        self.mrps += rhs.mrps;
        self.lat_us_50 += rhs.lat_us_50;
        self.lat_us_99 += rhs.lat_us_99;
    }
}

impl Stats {
    fn new() -> Self {
        Self::default()
    }

    fn header_str() -> &'static str {
        "mrps lat_us_50 lat_us_99"
    }

    fn to_row(&self) -> String {
        format!("{} {} {}", self.mrps, self.lat_us_50, self.lat_us_99)
    }
}

fn print_stats(client: &mut Client, stats: &[ArcSwap<CachePadded<Stats>>], thread_id: usize) {
    let dura = client.tput_t0.elapsed();
    let tput_mrps = client.num_resps_tot as f64 / dura.as_secs_f64() / 1e6;
    let lat_us_50 = client.point_latency.perc(0.50) as f64 / 10.0;
    let lat_us_99 = client.point_latency.perc(0.99) as f64 / 10.0;
    let range_lat_us_99 = client.range_latency.perc(0.99);

    stats[thread_id].store(Arc::new(CachePadded::new(Stats {
        mrps: tput_mrps,
        lat_us_50,
        lat_us_99,
    })));

    println!(
        "Client {thread_id}. Tput = {tput_mrps:.03} Mrps. \
         Point Latency (us) = {lat_us_50:.02}, {lat_us_99:.02}. \
         Range Latency (us) = {range_lat_us_99}"
    );

    if thread_id == 0 {
        let mut accum = Stats::new();
        for s in stats {
            // arc_swap::Guard -> Arc -> CachePadded -> Stats
            accum += ***s.load();
        }
        accum.lat_us_50 /= stats.len() as f64;
        accum.lat_us_99 /= stats.len() as f64;
        // write accum to file
        println!("{}", accum.to_row());
    }

    client.num_resps_tot = 0;
    client.point_latency.reset();
    client.range_latency.reset();
    client.tput_t0 = Instant::now();
}

fn run_client(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    // TODO(cjr): Note that eRPC also bind threads to core to reduce latency and jitters
    let mut stats = Vec::new();
    stats.resize_with(opt.num_client_threads, || {
        ArcSwap::from_pointee(CachePadded::new(Stats::new()))
    });

    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for i in 0..opt.num_client_threads {
            let tid = i;
            let opt = &opt;
            let stats = &stats;
            handles.push(s.spawn(move || {
                let mut client = Client::new();

                // TODO(cjr): eRPC sets up a dedicate connection to a server thread from each
                // client to balance the load. This is not a common operation in other RPC
                // frameworks (establishing connections to a particular server thread).
                let stub = MasstreeAnalyticsClient::connect((
                    opt.server_addr.as_deref().unwrap_or("127.0.0.1"),
                    opt.server_port + (tid % opt.num_server_fg_threads) as u16,
                ))?;
                log::info!("main: Thread {}: Connected. Sending requests.", tid);

                let mut req_ts = Vec::with_capacity(opt.req_window);
                req_ts.resize_with(opt.req_window, || Instant::now());

                let workload = generate_workload(&opt);
                let local_ex = smol::LocalExecutor::new();

                smol::block_on(local_ex.run(async {
                    let start = Instant::now();
                    let dura = Duration::from_millis(opt.test_ms);
                    let mut timer = Instant::now();
                    let print_period = Duration::from_millis(PRINT_STATS_PERIOD_MS);

                    let mut point_resp = FuturesUnordered::new();
                    let mut range_resp = FuturesUnordered::new();
                    for query in &workload {
                        match query {
                            Query::Point(req) => {
                                req_ts[req.token().0] = Instant::now();
                                point_resp.push(local_ex.run(stub.query_point(req)))
                            }
                            Query::Range(req) => {
                                req_ts[req.token().0] = Instant::now();
                                range_resp.push(local_ex.run(stub.query_range(req)));
                            }
                        }
                    }

                    loop {
                        select! {
                            resp = point_resp.next() => {
                                if resp.is_some() {
                                    let rref = resp.unwrap()?;
                                    let req_id = rref.token().0;
                                    // scale up to fit in the bucket
                                    let usec = (req_ts[req_id].elapsed() * 10).as_micros() as usize;
                                    client.num_resps_tot += 1;
                                    client.point_latency.update(usec);
                                    if let Query::Point(req) = &workload[req_id] {
                                        req_ts[req_id] = Instant::now();
                                        point_resp.push(local_ex.run(stub.query_point(req)));
                                    }
                                }
                            }
                            resp = range_resp.next() => {
                                if resp.is_some() {
                                    let rref = resp.unwrap()?;
                                    let req_id = rref.token().0;
                                    // scale up to fit in the bucket
                                    let usec = (req_ts[req_id].elapsed() * 10).as_micros() as usize;
                                    client.num_resps_tot += 1;
                                    client.range_latency.update(usec);
                                    if let Query::Range(req) = &workload[req_id] {
                                        req_ts[req_id] = Instant::now();
                                        range_resp.push(local_ex.run(stub.query_range(req)));
                                    }
                                }
                            }
                            default => {
                                if TERMINATE.load(Ordering::Relaxed) {
                                    break;
                                }
                                if start.elapsed() > dura {
                                    break;
                                }
                                if timer.elapsed() > print_period {
                                    timer = Instant::now();
                                    print_stats(&mut client, &stats, tid);
                                }
                            }
                        };
                    }

                    Result::<(), mrpc::Status>::Ok(())
                }))
            }));
        }
    });

    Ok(())
}

fn main() {
    logging::init_env_log("RUST_LOG", "debug");
    fastrand::seed(42);

    // register sigint handler
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe { signal::sigaction(signal::SIGINT, &sig_action) }
        .expect("failed to register sighandler");

    let opt = Opt::from_args();
    log::info!("masstree_analytics options: {:?}", opt);

    assert!(
        opt.req_window <= APP_MAX_REQ_WINDOW,
        "Invalid req window, {} vs {}",
        opt.req_window,
        APP_MAX_REQ_WINDOW
    );
    assert!(
        opt.range_req_percent <= 100,
        "Invalid range req percent: {}",
        opt.range_req_percent
    );

    run_client(opt).unwrap();
}
