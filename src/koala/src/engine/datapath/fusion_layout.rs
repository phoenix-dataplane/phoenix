use interface::rpc::RpcId;
use interface::Handle;
use std::ptr;

/*
 * Warning: bit constraints must be followed!
 */

pub const PAGE_SIZE: usize = 4096;
pub const MAX_SGMT: usize = 64;
pub const HEAD_META_LEN: usize = 4;

/// Format:
/// | rpc_ending_count | sge_count | meta_offset | data... |   sge_len... | rpc_ending_index... |
/// |         1        |     1     |       2     | ...     |     2\*sge   |       1\*rpc        |
/// Even though the page is finished, if it is not reclaimed, all data inside it should be valid.
pub struct BufferPage {
    data: [u8; PAGE_SIZE],
    /// current offset from self.data (which means the length of metadata is included)
    current_offset: u16,
    // The following fields will be written into data
    ending_count: u8,
    sge_count: u8,
    sge_len_arr: [u16; MAX_SGMT],
    ending_index_arr: [u8; MAX_SGMT],
    // share length with ending_index_arr
    finished_rpc_arr: [RpcId; MAX_SGMT],
}

impl BufferPage {
    fn meta_calc(rpc_ending_count: u8, sge_count: u8) -> u16 {
        HEAD_META_LEN as u16 + 2 * sge_count as u16 + rpc_ending_count as u16
    }

    pub fn new() -> Self {
        BufferPage {
            data: [0xcc; PAGE_SIZE],
            current_offset: HEAD_META_LEN as u16,
            ending_count: 0,
            sge_count: 0,
            sge_len_arr: [u16::MAX; MAX_SGMT],
            ending_index_arr: [u8::MAX; MAX_SGMT],
            finished_rpc_arr: [RpcId::new(Handle(u32::MAX), u32::MAX, 0); MAX_SGMT],
        }
    }

    pub fn safe_to_hold_two(total_length: usize, ending_count: u8) -> bool {
        Self::meta_calc(ending_count, 2) + total_length as u16 <= PAGE_SIZE as u16
    }

    #[inline(always)]
    pub fn page_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    pub fn can_hold(&self, len: usize, is_rpc_ending: bool) -> bool {
        (self.sge_count as usize) < MAX_SGMT
            && (self.current_offset - HEAD_META_LEN as u16
                + Self::meta_calc(is_rpc_ending as u8, self.sge_count)
                + len as u16)
                <= PAGE_SIZE as u16
    }

    pub fn copy_in(&mut self, range: ipc::buf::Range, rpc_ending: Option<RpcId>) {
        assert!(self.can_hold(range.len as usize, rpc_ending.is_some()));
        unsafe {
            ptr::copy_nonoverlapping(
                range.offset as *const u8,
                self.data.as_mut_ptr().add(self.current_offset as usize),
                range.len as usize,
            )
        };
        if rpc_ending.is_some() {
            self.ending_index_arr[self.ending_count as usize] = self.sge_count;
            self.finished_rpc_arr[self.ending_count as usize] = rpc_ending.unwrap();
            self.ending_count += 1;
        }
        self.current_offset += range.len as u16;
        self.sge_len_arr[self.sge_count as usize] = range.len as u16;
        self.sge_count += 1;
    }

    pub fn finish(&mut self) -> ipc::buf::Range {
        let meta_ptr = self.page_ptr() as *mut u8;

        // meta head
        unsafe {
            *meta_ptr = self.ending_count as u8;
            *(meta_ptr.add(1)) = self.sge_count as u8;
            *((meta_ptr as *mut u16).add(1)) = self.current_offset; // meta_offset
        }
        let meta_body_ptr =
            unsafe { (self.page_ptr() as *mut u8).add(self.current_offset as usize) };
        unsafe {
            ptr::copy_nonoverlapping(
                self.sge_len_arr.as_ptr() as *const u8,
                meta_body_ptr,
                2 * self.sge_count as usize,
            );
            if self.ending_count > 0 {
                ptr::copy_nonoverlapping(
                    self.ending_index_arr.as_ptr() as *const u8,
                    meta_body_ptr.add(2 * self.sge_count as usize),
                    self.ending_count as usize,
                );
            }
        }
        ipc::buf::Range {
            offset: self.page_ptr() as u64,
            len: (self.current_offset + 2 * self.sge_count as u16 + self.ending_count as u16)
                as u64,
        }
    }

    pub fn associated_rpcs(&self) -> std::slice::Iter<RpcId> {
        (&self.finished_rpc_arr[0..(self.ending_count as usize)]).iter()
    }

    pub fn reset(&mut self) {
        self.current_offset = HEAD_META_LEN as u16;
        self.ending_count = 0;
        self.sge_count = 0;
    }
}

mod sa {
    use super::*;
    use static_assertions::const_assert;

    const_assert!(HEAD_META_LEN + MAX_SGMT * 4 <= PAGE_SIZE);
    const_assert!(PAGE_SIZE <= u16::MAX as usize);
}
