/// Control path commands.
use serde::{Serialize, Deserialize};


#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    NewClient,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(String),
}