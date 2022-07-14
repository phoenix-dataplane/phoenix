use std::mem;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};

use fasthash::city;
use nix::sys::signal;
use structopt::StructOpt;

use ::masstree_analytics::mt_index::{threadinfo_purpose, MtIndex, ThreadInfo};
use mrpc::{RRef, WRef};

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
    test_ms: usize,
    /// Number of eRPC processes in the cluster
    #[structopt(long)]
    num_processes: usize,
    /// The global ID of this process
    #[structopt(long)]
    process_id: usize,
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
        todo!();

        // Ok(res)
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
        let _server = mrpc::stub::Server::bind("0.0.0.0:5000")?
            .add_service(MasstreeAnalyticsServer::new(service))
            .serve()
            .await?;
        Ok(())
    })
}

fn run_client(opt: Opt) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

fn main() {
    init_env_log("RUST_LOG", "debug");

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
