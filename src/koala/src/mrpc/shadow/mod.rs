//! Shadow data types have identical memory layouts with mRPC data types in the
//! userland library, but they expose the details for marshaling and only
//! implement minimal necessary functions.

pub(crate) mod raw_vec;
pub(crate) mod vec;
