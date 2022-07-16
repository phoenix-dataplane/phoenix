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

use ::masstree_analytics::mt_index::{threadinfo_purpose, MtIndex, ThreadInfo};
use mrpc::{RRef, Token, WRef};

// Workload params
const BYPASS_MASSTREE: bool = false;
const APP_MAX_REQ_WINDOW: usize = 8;

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigint(sig: i32) {
    assert_eq!(sig, signal::SIGINT as i32);
    TERMINATE.store(true, Ordering::Relaxed);
}

#[derive(Debug, StructOpt)]
struct Opt {
    /// Number of server foreground threads
    #[structopt(long)]
    num_server_fg_threads: usize,
    /// Number of server background threads
    #[structopt(long)]
    num_server_bg_threads: usize,
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

    // === App common flags in apps/apps_common.h ===
    /// Test milliseconds
    #[structopt(long)]
    test_ms: u64,
    // /// Number of eRPC processes in the cluster
    // #[structopt(long)]
    // num_processes: usize,
    /// The global ID of this process
    #[structopt(long)]
    process_id: usize,
    /// Server address, could be an IP address or domain name. If not specified, server will use
    /// 0.0.0.0 and client will use 127.0.0.1
    #[structopt(long)]
    server_addr: Option<String>,
    /// Server port
    #[structopt(long)]
    server_port: u16,
}

impl Opt {
    fn is_server(&self) -> bool {
        self.process_id == 0
    }
}

pub mod masstree_analytics {
    mrpc::include_proto!("masstree_analytics");
}

use crate::masstree_analytics::masstree_analytics_server::{
    MasstreeAnalytics, MasstreeAnalyticsServer,
};
use crate::masstree_analytics::{
    PointRequest, PointResponse, QueryResult, RangeRequest, RangeResponse,
};

use crate::masstree_analytics::masstree_analytics_client::MasstreeAnalyticsClient;

#[derive(Debug)]
struct MasstreeAnalyticsService {
    mti: MtIndex,
    ti_vec: Vec<ThreadInfo>,
    dummy_point_response: WRef<PointResponse>,
}

impl MasstreeAnalyticsService {
    fn new(mti: MtIndex, ti_vec: Vec<ThreadInfo>) -> Self {
        Self {
            mti,
            ti_vec,
            dummy_point_response: WRef::new(PointResponse {
                result: QueryResult::NotFound.into(),
                value: 0,
            }),
        }
    }
}

#[mrpc::async_trait]
impl MasstreeAnalytics for MasstreeAnalyticsService {
    async fn query_point<'s>(
        &self,
        req: RRef<'s, PointRequest>,
    ) -> Result<WRef<PointResponse>, mrpc::Status> {
        log::trace!("point request: {:?}", req);

        #[allow(unreachable_code)]
        if BYPASS_MASSTREE {
            // Send a garbage response
            return Ok(self.dummy_point_response.clone());
        }

        let mut value = 0;
        let found = self.mti.get(req.key as usize, &mut value, self.ti_vec[0]);
        let result = if found {
            QueryResult::Found
        } else {
            QueryResult::NotFound
        };

        let resp = WRef::new(PointResponse {
            result: result.into(),
            value: value as _,
        });

        Ok(resp)
    }

    async fn query_range<'s>(
        &self,
        req: RRef<'s, RangeRequest>,
    ) -> Result<WRef<RangeResponse>, mrpc::Status> {
        log::trace!("range request: {:?}", req);

        let count = self
            .mti
            .sum_in_range(req.key as usize, req.range as usize, self.ti_vec[0]);

        let resp = WRef::new(RangeResponse {
            result: QueryResult::Found.into(),
            range_count: count as _,
        });

        Ok(resp)
    }
}

fn run_server(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    // Create the Masstree using the main thread and insert keys
    let ti = ThreadInfo::new(threadinfo_purpose::TI_MAIN, -1);
    let mti = MtIndex::new(ti);

    for i in 0..opt.num_keys {
        // eRPC uses city hash
        let key = city::hash64(unsafe {
            slice::from_raw_parts(&i as *const _ as *const u8, mem::size_of_val(&i))
        }) as usize;
        let value = i;
        mti.put(key, value, ti);
    }

    // Create Masstree threadinfo structs for server threads
    let total_server_threads = opt.num_server_fg_threads + opt.num_server_bg_threads;
    let mut ti_vec = Vec::with_capacity(total_server_threads);
    for i in 0..total_server_threads {
        ti_vec.push(ThreadInfo::new(threadinfo_purpose::TI_PROCESS, i as _));
    }

    // mRPC stuff
    // TODO(cjr): eRPC spawns a bunch of foreground and background threads for point and range
    // query handler respectively. This design is reasonable because user can usually tell whether
    // a service is going to block.
    //
    // We should add an add_service_blocking() API for blocking tasks (e.g., range query).
    smol::block_on(async {
        let service = MasstreeAnalyticsService::new(mti, ti_vec);
        let _server = mrpc::stub::Server::bind((
            opt.server_addr.as_deref().unwrap_or("0.0.0.0"),
            opt.server_port,
        ))?
        .add_service(MasstreeAnalyticsServer::new(service))
        .serve()
        .await?;
        Ok(())
    })
}

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
                        if TERMINATE.load(Ordering::Relaxed) {
                            break;
                        }
                        if start.elapsed() > dura {
                            break;
                        }

                        select! {
                            resp = point_resp.next() => {
                                let rref = resp.unwrap()?;
                                let token = rref.token();
                                if let Query::Point(req) = &workload[token.0] {
                                    point_resp.push(local_ex.run(client.query_point(req)));
                                }
                            }
                            resp = range_resp.next() => {
                                let rref = resp.unwrap()?;
                                let token = rref.token();
                                if let Query::Range(req) = &workload[token.0] {
                                    range_resp.push(local_ex.run(client.query_range(req)));
                                }
                            }
                        };
                        // let resp = responses.next().await.unwrap()?;
                        // match resp {
                        //     PointResponse { result, value } => {}
                        //     RangeResponse {
                        //         result,
                        //         range_count,
                        //     } => {}
                        // }
                    }

                    Result::<(), mrpc::Status>::Ok(())
                }))
            }));
        }
    });

    Ok(())
}

fn main() {
    init_env_log("RUST_LOG", "debug");
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

    if opt.num_server_bg_threads == 0 {
        log::warn!("main: No background threads. Range queries will run in foreground.");
    }

    if opt.is_server() {
        run_server(opt).unwrap();
    } else {
        run_client(opt).unwrap();
    }
}

fn init_env_log(filter_env: &str, default_level: &str) {
    use chrono::Utc;
    use std::io::Write;

    let env = env_logger::Env::new().filter_or(filter_env, default_level);
    env_logger::Builder::from_env(env)
        .format(|buf, record| {
            let level_style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "[{} {} {}:{}] {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.6f"),
                level_style.value(record.level()),
                record.file().unwrap_or("<unnamed>"),
                record.line().unwrap_or(0),
                &record.args()
            )
        })
        .init();

    log::info!("env_logger initialized");
}
