use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use futures::StreamExt;
use geo::algorithm::haversine_distance::HaversineDistance;
use geo::point;
use kdtree::KdTree;
use minstant::Instant;
use mongodb::Database;

use mrpc::alloc::{String as MrpcString, Vec};
use mrpc::{RRef, WRef};

use super::db::Point;
use super::tracer::Tracer;

pub mod hotel_microservices {
    pub mod geo {
        // The string specified here must match the proto package name
        mrpc::include_proto!("geo");
    }
}

use hotel_microservices::geo::geo_server::Geo;
use hotel_microservices::geo::{Request as GeoRequest, Result as GeoResult};

pub struct GeoService {
    db_handle: Database,
    index: Option<KdTree<f64, String, [f64; 2]>>,
    log_path: Option<PathBuf>,
    tracer: RefCell<Tracer>,
}

// SAFETY: This is unsafe
unsafe impl Send for GeoService {}
unsafe impl Sync for GeoService {}

impl Drop for GeoService {
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
impl Geo for GeoService {
    async fn nearby(&self, request: RRef<GeoRequest>) -> Result<WRef<GeoResult>, mrpc::Status> {
        let start = Instant::now();
        let nearest = self
            .get_nearby_points(request.lat.into(), request.lon.into())
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;
        self.tracer
            .borrow_mut()
            .record_proc("geo", start.elapsed())
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;

        log::trace!("geo after getNearbyPoints, len = {}", nearest.len());
        let reply = GeoResult { hotel_ids: nearest };

        let wref = WRef::new(reply);
        Ok(wref)
    }
}

#[inline]
fn haversine_distance(left: &[f64], right: &[f64]) -> f64 {
    let left = point!(x: left[0], y: left[1]);
    let right = point!(x: right[0], y: right[1]);
    left.haversine_distance(&right)
}

const MAX_SEARCH_RADIUS: f64 = 10_000f64;
const MAX_SEARCH_RESULTS: usize = 5;

impl GeoService {
    fn get_nearby_points(&self, lat: f64, lon: f64) -> Result<Vec<MrpcString>> {
        log::trace!("In geo getNearbyPoints, lat = {:.4}, lon = {:.4}", lat, lon);

        let center = [lat, lon];
        let index = self
            .index
            .as_ref()
            .ok_or(anyhow!("KdTree index is not built"))?;
        let nearest = index.iter_nearest(&center, &haversine_distance)?;
        let mut points = Vec::with_capacity(MAX_SEARCH_RESULTS);
        for (i, (dist, id)) in nearest.enumerate() {
            if dist > MAX_SEARCH_RADIUS || i == MAX_SEARCH_RESULTS {
                break;
            } else {
                points.push(id.into());
            }
        }
        Ok(points)
    }

    async fn set_geo_index(&mut self) -> Result<()> {
        const DIMENSIONS: usize = 2;
        log::trace!("new geo newGeoIndex");

        let mut kdtree = KdTree::new(DIMENSIONS);
        let collection = self.db_handle.collection::<Point>("geo");
        let mut points = collection.find(None, None).await?;
        while let Some(point) = points.next().await {
            let point = point?;
            kdtree.add([point.lat, point.lon], point.id)?;
        }
        self.index = Some(kdtree);
        Ok(())
    }
}

impl GeoService {
    pub async fn new(db: Database, log_path: Option<PathBuf>) -> Result<GeoService> {
        let mut tracer = Tracer::new();
        tracer.new_proc_entry("geo");
        let mut service = GeoService {
            db_handle: db,
            index: None,
            log_path,
            tracer: RefCell::new(tracer),
        };
        service.set_geo_index().await?;
        Ok(service)
    }
}
