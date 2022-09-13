use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;

use anyhow::{anyhow, Result};
use futures::poll;
use hyper::{Body, Request, Response, StatusCode};
use minstant::Instant;
use serde_json::json;

use mrpc::RRef;

use super::tracer::Tracer;

pub mod hotel_microservices {
    pub mod search {
        // The string specified here must match the proto package name
        mrpc::include_proto!("search");
    }
    pub mod profile {
        // The string specified here must match the proto package name
        mrpc::include_proto!("profile");
    }
}

use hotel_microservices::profile::profile_client::ProfileClient;
use hotel_microservices::profile::{Request as ProfileRequest, Result as ProfileResult};
use hotel_microservices::search::search_client::SearchClient;
use hotel_microservices::search::NearbyRequest as SearchRequest;

pub struct FrontendService {
    search_client: SearchClient,
    profile_client: ProfileClient,
    log_path: Option<PathBuf>,
    tracer: RefCell<Tracer>,
}

// SAFETY: This is unsafe
unsafe impl Send for FrontendService {}
unsafe impl Sync for FrontendService {}

impl FrontendService {
    pub fn new(search: SearchClient, profile: ProfileClient, log_path: Option<PathBuf>) -> Self {
        let mut tracer = Tracer::new();
        tracer.new_end_to_end_entry("search");
        tracer.new_end_to_end_entry("profile");
        FrontendService {
            search_client: search,
            profile_client: profile,
            log_path,
            tracer: RefCell::new(tracer),
        }
    }
}

impl Drop for FrontendService {
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

impl FrontendService {
    async fn handle_search(&self, request: Request<Body>) -> Result<Response<Body>> {
        let params = request
            .uri()
            .query()
            .map(|v| url::form_urlencoded::parse(v.as_bytes()).collect::<HashMap<_, _>>())
            .ok_or(anyhow!("no query in request"))?;

        let in_date = params
            .get("inDate")
            .ok_or(anyhow!("inDate param not found in query"))?;
        let out_date = params
            .get("outDate")
            .ok_or(anyhow!("outDate param not found in query"))?;

        let s_lat = params
            .get("lat")
            .ok_or(anyhow!("outDate param not found in query"))?;
        let s_lon = params
            .get("lon")
            .ok_or(anyhow!("outDate param not found in query"))?;
        let lat = s_lat.parse()?;
        let lon = s_lon.parse()?;

        let locale = params.get("locale").map(|x| x.as_ref()).unwrap_or("en");

        let search_req = SearchRequest {
            lat,
            lon,
            in_date: in_date.as_ref().into(),
            out_date: out_date.as_ref().into(),
        };
        log::trace!("SEARCH {:?}", search_req);

        let start = Instant::now();
        let mut resp_fut = self.search_client.nearby(search_req);
        let result = loop {
            let result = poll!(&mut resp_fut);
            match result {
                Poll::Ready(resp) => break resp,
                Poll::Pending => {}
            }
        }?;
        self.tracer
            .borrow_mut()
            .record_end_to_end("search", start.elapsed())?;
        log::trace!("SearchHandler gets searchResp");

        let profile_req = ProfileRequest {
            hotel_ids: result.hotel_ids.clone(),
            locale: locale.into(),
        };

        let start = Instant::now();
        let mut resp_fut = self.profile_client.get_profiles(profile_req);
        let result = loop {
            let result = poll!(&mut resp_fut);
            match result {
                Poll::Ready(resp) => break resp,
                Poll::Pending => {}
            }
        }?;
        self.tracer
            .borrow_mut()
            .record_end_to_end("profile", start.elapsed())?;
        log::trace!("searchHandler gets profileResp");

        let response_json = geo_json_response(result)?;
        let response = Response::builder()
            .status(200)
            .header("Access-Control-Allow-Origin", "*")
            .body(response_json.into())?;
        Ok(response)
    }
}

fn geo_json_response<'s>(res: RRef<'s, ProfileResult>) -> Result<String> {
    let mut hotels = Vec::with_capacity(res.hotels.len());
    for hotel in res.hotels.iter() {
        let id = hotel.id.as_str();
        let name = hotel.name.as_str();
        let phone_number = hotel.phone_number.as_str();
        let lat = hotel
            .address
            .as_ref()
            .ok_or(anyhow!("latitude not found in profile"))?
            .lat;
        let lon = hotel
            .address
            .as_ref()
            .ok_or(anyhow!("longitude not found in profile"))?
            .lon;
        let hotel_json = json!({
            "type": "Feature",
            "id": id,
            "properties": {
                "name": name,
                "phone_number": phone_number,
            },
            "geometry": {
                "type": "Point",
                "coordinates": [lat, lon],
            },
        });
        hotels.push(hotel_json);
    }

    let response = json!({
        "type": "FeatureCollection",
        "features": hotels,
    });
    Ok(response.to_string())
}

pub async fn dispatch_fn(
    frontend: Arc<FrontendService>,
    request: Request<Body>,
) -> Result<Response<Body>, hyper::Error> {
    match request.uri().path() {
        "/hotels" => {
            let response = frontend.handle_search(request).await;
            match response {
                Ok(resp) => Ok(resp),
                Err(err) => {
                    let body = format!("Internal error: {}", err.to_string()).into();
                    let mut resp = Response::new(body);
                    *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    Ok(resp)
                }
            }
        }
        _ => {
            let mut not_found = Response::new(Body::empty());
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}
