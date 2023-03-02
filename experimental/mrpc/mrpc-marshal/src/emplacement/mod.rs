#![allow(unused)]
#![allow(clippy::missing_safety_doc)]

pub mod bytes;
pub mod message;
mod numeric;
pub mod string;

pub use numeric::{bool, double, float, int32, int64, uint32, uint64};
