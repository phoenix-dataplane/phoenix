//! Control path commands.
use serde::{Deserialize, Serialize};

use engine::SchedulingMode;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),
}
