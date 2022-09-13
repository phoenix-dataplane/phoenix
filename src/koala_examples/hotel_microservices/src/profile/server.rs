use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::{bail, Result};
use memcache::Client as MemcacheClient;
use minstant::Instant;
use mongodb::bson::doc;
use mongodb::sync::Database;

use mrpc::alloc::Vec;
use mrpc::{RRef, WRef};

use super::db;
use super::tracer::Tracer;

pub mod hotel_microservices {
    pub mod profile {
        // The string specified here must match the proto package name
        mrpc::include_proto!("profile");
    }
}

use hotel_microservices::profile::profile_server::Profile;
use hotel_microservices::profile::{Address, Hotel};
use hotel_microservices::profile::{Request as ProfileRequest, Result as ProfileResult};

impl From<db::Hotel> for Hotel {
    fn from(hotel: db::Hotel) -> Self {
        Hotel {
            id: hotel.id.into(),
            name: hotel.name.into(),
            phone_number: hotel.phone_number.into(),
            description: hotel.description.into(),
            address: Some(hotel.address.into()),
            images: Vec::new(),
        }
    }
}

impl From<db::Address> for Address {
    fn from(addr: db::Address) -> Self {
        Address {
            street_number: addr.street_number.into(),
            street_name: addr.street_name.into(),
            city: addr.city.into(),
            state: addr.state.into(),
            country: addr.country.into(),
            postal_code: addr.postal_code.into(),
            lat: addr.lat,
            lon: addr.lon,
        }
    }
}

pub struct ProfileService {
    memc_client: MemcacheClient,
    db_handle: Database,
    log_path: Option<PathBuf>,
    tracer: RefCell<Tracer>,
}

// SAFETY: This is unsafe
unsafe impl Send for ProfileService {}
unsafe impl Sync for ProfileService {}

impl Drop for ProfileService {
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
impl Profile for ProfileService {
    async fn get_profiles<'s>(
        &self,
        request: RRef<'s, ProfileRequest>,
    ) -> Result<WRef<ProfileResult>, mrpc::Status> {
        let start = Instant::now();
        let result = self
            .get_profiles_internal(request)
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;
        self.tracer
            .borrow_mut()
            .record_proc("profile", start.elapsed())
            .map_err(|err| mrpc::Status::internal(err.to_string()))?;

        let wref = WRef::new(result);
        Ok(wref)
    }
}

impl ProfileService {
    #[inline]
    fn get_profiles_internal<'s>(
        &self,
        request: RRef<'s, ProfileRequest>,
    ) -> Result<ProfileResult> {
        let mut hotels = Vec::with_capacity(request.hotel_ids.len());
        for hotel_id in request.hotel_ids.iter() {
            let item: Option<String> = self.memc_client.get(hotel_id.as_str())?;
            if let Some(item) = item {
                log::trace!("memc hit with {}", item);
                let hotel_prof: db::Hotel = serde_json::from_str(&item)?;
                let hotel_proto = hotel_prof.into();
                hotels.push(hotel_proto);
            } else {
                let collection = self.db_handle.collection::<db::Hotel>("hotels");
                let hotel = collection.find_one(doc! { "id": hotel_id.as_str() }, None)?;
                if let Some(hotel) = hotel {
                    let hotel_json = serde_json::to_string(&hotel)?;
                    let hotel_proto = hotel.into();
                    hotels.push(hotel_proto);
                    self.memc_client.set(hotel_id.as_str(), hotel_json, 0)?;
                } else {
                    bail!("hotel {} not found", hotel_id);
                }
            }
        }

        let result = ProfileResult { hotels };
        Ok(result)
    }
}

impl ProfileService {
    pub fn new(db: Database, memc: MemcacheClient, log_path: Option<PathBuf>) -> Self {
        let mut tracer = Tracer::new();
        tracer.new_proc_entry("profile");
        ProfileService {
            memc_client: memc,
            db_handle: db,
            log_path,
            tracer: RefCell::new(tracer),
        }
    }
}
