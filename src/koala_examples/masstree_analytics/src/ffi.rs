// Keep this so that we can correctly link with stdc++.
#[allow(unused_imports)]
use link_cplusplus;

// NOTE(cjr): Need to link two libraries here. This is VERY tricky!
#[link(name = "mt_index", kind = "static")]
#[link(name = "masstree", kind = "static")]
extern "C" {
    // MtIndex API
    pub fn mt_index_create() -> *mut ();
    pub fn mt_index_destroy(obj: *mut ());
    pub fn mt_index_setup(obj: *mut (), ti: *mut ());
    pub fn mt_index_put(obj: *mut (), key: usize, value: usize, ti: *mut ());
    pub fn mt_index_get(obj: *mut (), key: usize, value: &mut usize, ti: *mut ()) -> bool;
    pub fn mt_index_sum_in_range(obj: *mut (), cur_key: usize, range: usize, ti: *mut ()) -> usize;

    // threadinfo API
    pub fn threadinfo_make(purpose: i32, index: i32) -> *mut ();
}

pub mod threadinfo_purpose {
    pub type Type = ::std::os::raw::c_uint;
    pub const TI_MAIN: Type = 0;
    pub const TI_PROCESS: Type = 1;
    pub const TI_LOG: Type = 2;
    pub const TI_CHECKPOINT: Type = 3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_mt_index() {
        // calling the function from mt_index library
        unsafe {
            let _mt_index = mt_index_create();
        };
    }

    #[test]
    fn test_link_threadinfo() {
        // calling the function from mt_index library
        unsafe {
            let _ti = threadinfo_make(0, -1);
        };
    }
}
