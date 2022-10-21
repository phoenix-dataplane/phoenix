#![allow(clippy::module_inception)]
use thiserror::Error;

pub mod prost;
pub mod service;

pub use prost::Builder;
pub use prost::{compile_protos, configure};

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Serde JSON Error: {0}")]
    SerdeJSON(#[from] serde_json::Error),
}
