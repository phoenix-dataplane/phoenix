use crate::ffi;

// Re-exports
pub use ffi::threadinfo_purpose;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadInfo(pub(crate) *mut ());

unsafe impl Send for ThreadInfo {}
unsafe impl Sync for ThreadInfo {}

impl ThreadInfo {
    pub fn new(purpose: ffi::threadinfo_purpose::Type, index: i32) -> Self {
        let ptr = unsafe { ffi::threadinfo_make(purpose as i32, index) };
        Self(ptr)
    }
}

#[derive(Debug, Clone)]
pub struct MtIndex(*mut ());

unsafe impl Send for MtIndex {}
unsafe impl Sync for MtIndex {}

impl MtIndex {
    pub fn new(ti: ThreadInfo) -> Self {
        let ptr = unsafe { ffi::mt_index_create() };
        unsafe { ffi::mt_index_setup(ptr, ti.0) };
        Self(ptr)
    }

    #[inline]
    pub fn put(&self, key: usize, value: usize, ti: ThreadInfo) {
        unsafe {
            ffi::mt_index_put(self.0, key, value, ti.0);
        }
    }

    #[inline]
    pub fn get(&self, key: usize, value: &mut usize, ti: ThreadInfo) -> bool {
        unsafe { ffi::mt_index_get(self.0, key, value, ti.0) }
    }

    #[inline]
    pub fn sum_in_range(&self, cur_key: usize, range: usize, ti: ThreadInfo) -> usize {
        unsafe { ffi::mt_index_sum_in_range(self.0, cur_key, range, ti.0) }
    }
}

impl Drop for MtIndex {
    fn drop(&mut self) {
        unsafe {
            ffi::mt_index_destroy(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_query() {
        // Create the Masstree using the main thread and insert keys
        let ti = ThreadInfo::new(threadinfo_purpose::TI_MAIN, -1);
        let mti = MtIndex::new(ti);

        for i in 0..1000 {
            let key = i;
            let value = i;
            mti.put(key, value, ti);
        }

        for i in 0..1000 {
            let key = i;
            let mut value = 0;
            let found = mti.get(key, &mut value, ti);
            assert!(found && value == i);
        }

        for i in 3456..3456 + 100 {
            let key = i;
            let mut value = 0;
            let found = mti.get(key, &mut value, ti);
            assert!(!found && value == 0);
        }
    }

    #[test]
    fn test_range_query() {
        // Create the Masstree using the main thread and insert keys
        let ti = ThreadInfo::new(threadinfo_purpose::TI_MAIN, -1);
        let mti = MtIndex::new(ti);

        for i in 0..1000 {
            let key = i;
            let value = i;
            mti.put(key, value, ti);
        }

        // test range
        let mut range = 1;
        for i in 0..1000 {
            let cur_key = i;
            range = range * 131 % 500;
            let sum = mti.sum_in_range(cur_key, range, ti);
            if i + range < 1000 {
                assert_eq!(sum, (i + i + range - 1) * range / 2);
            } else {
                assert_eq!(sum, (i + 999) * (1000 - i) / 2);
            }
        }
    }
}
