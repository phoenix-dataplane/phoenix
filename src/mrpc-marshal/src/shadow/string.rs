use std::fmt;
use std::ops;
use std::str;
use super::vec::Vec;

pub struct String {
    pub buf: Vec<u8>,
}
// pub use shm::string::String;

impl fmt::Debug for String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

// TODO(cjr): Maybe enforce Copy-on-access semantic here.
impl ops::Deref for String {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.buf) }
    }
}
