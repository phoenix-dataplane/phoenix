#![feature(scoped_threads)]
use std::mem;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fasthash::city;
use futures::select;
use futures::stream::{FuturesUnordered, StreamExt};
use minstant::Instant;
use nix::sys::signal;
use structopt::StructOpt;

use mrpc::{Token, WRef};

// Workload params
const APP_MAX_REQ_WINDOW: usize = 8;

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
fn get_random_key(num_keys: usize) -> usize {
    let generator_key = fastrand::usize(..num_keys);
    city::hash64(unsafe {
        slice::from_raw_parts(
            &generator_key as *const _ as *const u8,
            mem::size_of_val(&generator_key),
        )
    }) as _
}

// Helper function for clients
fn generate_workload(opt: &Opt) -> Vec<Query> {
    let mut workload = Vec::with_capacity(opt.req_window);
    for i in 0..opt.req_window {
        let key = get_random_key(opt.num_keys) as u64;
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

fn run_client(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    // TODO(cjr): Note that eRPC also bind threads to core to reduce latency and jitters
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for i in 0..opt.num_client_threads {
            let tid = i;
            let opt = &opt;
            handles.push(s.spawn(move || {
                // TODO(cjr): eRPC sets up a dedicate connection to a server thread from each
                // client to balance the load. This is not a common operation in other RPC
                // frameworks (establishing connections to a particular server thread).
                let client = MasstreeAnalyticsClient::connect((
                    opt.server_addr.as_deref().unwrap_or("127.0.0.1"),
                    opt.server_port,
                ))?;
                log::info!("main: Thread {}: Connected. Sending requests.", tid);

                let workload = generate_workload(&opt);
                let local_ex = smol::LocalExecutor::new();

                smol::block_on(local_ex.run(async {
                    let start = Instant::now();
                    let dura = Duration::from_millis(opt.test_ms);

                    let mut point_resp = FuturesUnordered::new();
                    let mut range_resp = FuturesUnordered::new();
                    for query in &workload {
                        match query {
                            Query::Point(req) => {
                                point_resp.push(local_ex.run(client.query_point(req)))
                            }
                            Query::Range(req) => {
                                range_resp.push(local_ex.run(client.query_range(req)));
                            }
                        }
                    }

                    loop {
                        // if TERMINATE.load(Ordering::Relaxed) {
                        //     break;
                        // }
                        // if start.elapsed() > dura {
                        //     break;
                        // }

                        select! {
                            resp = point_resp.next() => {
                                if resp.is_some() {
                                    let rref = resp.unwrap()?;
                                    let token = rref.token();
                                    if let Query::Point(req) = &workload[token.0] {
                                        point_resp.push(local_ex.run(client.query_point(req)));
                                    }
                                }
                            }
                            resp = range_resp.next() => {
                                if resp.is_some() {
                                    let rref = resp.unwrap()?;
                                    let token = rref.token();
                                    if let Query::Range(req) = &workload[token.0] {
                                        range_resp.push(local_ex.run(client.query_range(req)));
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

mod logging;

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
