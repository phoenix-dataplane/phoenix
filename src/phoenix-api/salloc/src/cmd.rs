//! salloc control path commands.
use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, phoenix_api::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    // Layout: (size, align)
    AllocShm(usize, usize),
    // addr: usize
    DeallocShm(usize),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CompletionKind {
    // remote_addr, file_off
    AllocShm(usize, i64),
    DeallocShm,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Completion(pub IResult<CompletionKind>);
