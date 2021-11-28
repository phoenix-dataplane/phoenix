//! Shared memory region.
#![cfg(feature = "koala")]
use std::io;
use std::ops::{Deref, DerefMut};

use memfd::{Memfd, MemfdOptions};
use memmap2::{MmapMut, MmapOptions};
use thiserror::Error;

use crate::{ffi, ibv, rdmacm};

use interface::{AccessFlags, RemoteKey};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
}

pub struct MemoryRegion {
    mr: *mut ffi::ibv_mr,
    mmap: MmapMut,
    memfd: Memfd,
}

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl Deref for MemoryRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.mmap[..]
    }
}

impl DerefMut for MemoryRegion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mmap[..]
    }
}

impl Drop for MemoryRegion {
    fn drop(&mut self) {
        let errno = unsafe { ffi::ibv_dereg_mr(self.mr) };
        if errno != 0 {
            let e = io::Error::from_raw_os_error(errno);
            panic!("{}", e);
        }
    }
}

impl MemoryRegion {
    pub fn new<'ctx>(
        pd: &'ctx ibv::ProtectionDomain<'ctx>,
        nbytes: usize,
        access: AccessFlags,
    ) -> Result<Self, Error> {
        let opts = MemfdOptions::default()
            .allow_sealing(true)
            .close_on_exec(false);
        let name = format!("shared-mr-{}", nbytes);
        let memfd = opts.create(name)?;
        memfd.as_file().set_len(nbytes as u64)?;

        let mut mmap = unsafe { MmapOptions::new().map_mut(memfd.as_file()) }?;

        let mr = unsafe {
            ffi::ibv_reg_mr(
                pd.pd,
                mmap.as_mut_ptr() as *mut _,
                nbytes as _,
                ibv::AccessFlags::from(access).0 .0 as _,
            )
        };

        if mr.is_null() {
            Err(Error::Io(io::Error::last_os_error()))
        } else {
            Ok(Self { mr, mmap, memfd })
        }
    }

    #[inline]
    pub fn mr(&self) -> *mut ffi::ibv_mr {
        self.mr
    }

    #[inline]
    pub fn handle(&self) -> u32 {
        unsafe { &*self.mr }.handle
    }

    #[inline]
    pub fn memfd(&self) -> &Memfd {
        &self.memfd
    }

    #[inline]
    pub fn rkey(&self) -> RemoteKey {
        RemoteKey(unsafe { &*self.mr }.rkey)
    }
}

impl From<&MemoryRegion> for rdmacm::MemoryRegion {
    fn from(other: &MemoryRegion) -> rdmacm::MemoryRegion {
        rdmacm::MemoryRegion(other.mr)
    }
}

impl From<&mut MemoryRegion> for rdmacm::MemoryRegion {
    fn from(other: &mut MemoryRegion) -> rdmacm::MemoryRegion {
        rdmacm::MemoryRegion(other.mr)
    }
}
