use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::Result;
use futures::StreamExt;
use memcache::Client as MemcacheClient;
use minstant::Instant;
use mongodb::bson::doc;
use mongodb::Database;

use mrpc::alloc::Vec;
use mrpc::{RRef, WRef};

use super::db;
use super::tracer::Tracer;

pub mod hotel_microservices {
    pub mod rate {
        // The string specified here must match the proto package name
        mrpc::include_proto!("rate");
    }
}

use hotel_microservices::rate::rate_server::Rate;
use hotel_microservices::rate::{RatePlan, RoomType};
use hotel_microservices::rate::{Request as RateRequest, Result as RateResult};

impl From<db::RoomType> for RoomType {
    fn from(room: db::RoomType) -> Self {
        RoomType {
            bookable_rate: room.bookable_rate,
            total_rate: room.total_rate,
            total_rate_inclusive: room.total_rate_inclusive,
            code: room.code.into(),
            currency: mrpc::alloc::String::default(),
            room_description: room.room_description.into(),
        }
    }
}

impl From<db::RatePlan> for RatePlan {
    fn from(plan: db::RatePlan) -> Self {
        RatePlan {
            hotel_id: plan.hotel_id.into(),
            code: plan.code.into(),
            in_date: plan.in_date.into(),
            out_date: plan.out_date.into(),
            room_type: Some(plan.room_type.into()),
        }
    }
}

pub struct RateService {
    memc_client: MemcacheClient,
    db_handle: Database,
    log_path: Option<PathBuf>,
    tracer: RefCell<Tracer>,
}

// SAFETY: This is unsafe
unsafe impl Send for RateService {}
unsafe impl Sync for RateService {}

impl Drop for RateService {
    fn drop(&mut self) {
        let mut tracer = self.tracer.borrow_mut();
        if let Some(path) = &self.log_path {
            if let Some(parent) = path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    log::error!("Error create logging dir: {}", err);
                }
            }
            if let Err(err) = tracer.to_csv(path) {
                log::error!("Error writting logs: {}", err);
            }
        }
    }
}

#[mrpc::async_trait]
impl Rate for RateService {
    async fn get_rates(
        &self,
        request: RRef<RateRequest>,
    ) -> Result<WRef<RateResult>, mrpc::Status> {
        let start = Instant::now();
        let result = self
            .get_rates_internal(request)
            .await
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;
        self.tracer
            .borrow_mut()
            .record_proc("rate", start.elapsed())
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;

        let wref = WRef::new(result);
        Ok(wref)
    }
}

impl RateService {
    async fn get_rates_internal(&self, request: RRef<RateRequest>) -> Result<RateResult> {
        let mut rate_plans = Vec::new();
        for hotel_id in request.hotel_ids.iter() {
            let item: Option<String> = self.memc_client.get(hotel_id.as_str())?;
            if let Some(item) = item {
                log::trace!("memc hit, hotelId = {}", hotel_id);
                let rate_strs = item.split("\n");
                for rate_str in rate_strs {
                    if !rate_str.is_empty() {
                        let rate_plan: db::RatePlan = serde_json::from_str(rate_str)?;
                        let proto_plan = rate_plan.into();
                        rate_plans.push(proto_plan);
                    }
                }
            } else {
                log::trace!("memc miss, hotelId = {}", hotel_id);
                let mut memc_str = String::new();
                let collections = self.db_handle.collection::<db::RatePlan>("inventory");
                let mut plans = collections
                    .find(doc! { "hotelId" : hotel_id.as_str() }, None)
                    .await?;
                while let Some(plan) = plans.next().await {
                    let plan = plan?;
                    let plan_json = serde_json::to_string(&plan)?;
                    memc_str += plan_json.as_str();
                    memc_str += "\n";
                    let proto_plan = plan.into();
                    rate_plans.push(proto_plan);
                }
                self.memc_client.set(hotel_id.as_str(), memc_str, 0)?;
            }
        }
        let result = RateResult { rate_plans };
        Ok(result)
    }
}

impl RateService {
    pub fn new(db: Database, memc: MemcacheClient, log_path: Option<PathBuf>) -> Self {
        let mut tracer = Tracer::new();
        tracer.new_proc_entry("rate");
        RateService {
            memc_client: memc,
            db_handle: db,
            log_path,
            tracer: RefCell::new(tracer),
        }
    }
}
