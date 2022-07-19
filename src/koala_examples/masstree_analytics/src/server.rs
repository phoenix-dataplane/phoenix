#![feature(scoped_threads)]
use std::mem;
use std::slice;

use fasthash::city;
use structopt::StructOpt;

use ::masstree_analytics::mt_index::{threadinfo_purpose, MtIndex, ThreadInfo};
use mrpc::{RRef, WRef};

// Workload params
const BYPASS_MASSTREE: bool = false;

#[derive(Debug, StructOpt)]
struct Opt {
    /// Number of server foreground threads
    #[structopt(long)]
    num_server_fg_threads: usize,
    /// Number of server background threads
    #[structopt(long)]
    num_server_bg_threads: usize,
    /// Number of keys in the server's Masstree
    #[structopt(long)]
    num_keys: usize,
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

use crate::masstree_analytics::masstree_analytics_server::{
    MasstreeAnalytics, MasstreeAnalyticsServer,
};
use crate::masstree_analytics::{
    PointRequest, PointResponse, QueryResult, RangeRequest, RangeResponse,
};

#[derive(Debug)]
struct MasstreeAnalyticsService {
    mti: MtIndex,
    tid: usize,
    ti_vec: Vec<ThreadInfo>,
    dummy_point_response: WRef<PointResponse>,
}

impl MasstreeAnalyticsService {
    fn new(mti: MtIndex, tid: usize, ti_vec: Vec<ThreadInfo>) -> Self {
        Self {
            mti,
            tid,
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
        log::trace!("point request: {:?} in mRPC thread {}", req, self.tid);

        #[allow(unreachable_code)]
        if BYPASS_MASSTREE {
            // Send a garbage response
            return Ok(self.dummy_point_response.clone());
        }

        let mut value = 0;
        let found = self.mti.get(req.key as usize, &mut value, self.ti_vec[self.tid]);
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
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for i in 0..opt.num_server_fg_threads {
            let tid = i;
            // cloning the reference
            let mti = mti.clone();
            // cloning the reference
            let ti_vec = ti_vec.clone();
            let opt = &opt;
            handles.push(s.spawn(move || {
                smol::block_on(async {
                    let service = MasstreeAnalyticsService::new(mti, tid, ti_vec);
                    let _server = mrpc::stub::Server::bind((
                        opt.server_addr.as_deref().unwrap_or("0.0.0.0"),
                        opt.server_port + tid as u16,
                    ))?
                    .add_service(MasstreeAnalyticsServer::new(service))
                    .serve()
                    .await?;
                    Result::<(), Box<dyn std::error::Error>>::Ok(())
                }).unwrap();
            }));
        }
    });

    Ok(())
}

mod logging;

fn main() {
    logging::init_env_log("RUST_LOG", "debug");

    let opt = Opt::from_args();
    log::info!("masstree_analytics options: {:?}", opt);

    if opt.num_server_bg_threads == 0 {
        log::warn!("main: No background threads. Range queries will run in foreground.");
    }

    run_server(opt).unwrap();
}
