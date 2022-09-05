use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(rename = "FrontendPort")]
    pub frontend_port: u16,

    #[serde(rename = "GeoAddress")]
    pub geo_addr: String,
    #[serde(rename = "GeoPort")]
    pub geo_port: u16,
    #[serde(rename = "GeoMongoAddress")]
    pub geo_mongo_addr: String,

    #[serde(rename = "ProfileAddress")]
    pub profile_addr: String,
    #[serde(rename = "ProfilePort")]
    pub profile_port: u16,
    #[serde(rename = "ProfileMongoAddress")]
    pub profile_mongo_addr: String,
    #[serde(rename = "ProfileMemcAddress")]
    pub profile_memc_addr: String,

    #[serde(rename = "RateAddress")]
    pub rate_addr: String,
    #[serde(rename = "RatePort")]
    pub rate_port: u16,
    #[serde(rename = "RateMongoAddress")]
    pub rate_mongo_addr: String,
    #[serde(rename = "RateMemcAddress")]
    pub rate_memc_addr: String,

    #[serde(rename = "SearchAddress")]
    pub search_addr: String,
    #[serde(rename = "SearchPort")]
    pub search_port: u16,

    #[serde(rename = "LogPath")]
    pub log_path: PathBuf,
}
