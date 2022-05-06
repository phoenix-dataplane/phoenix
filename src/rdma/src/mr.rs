//! Shared memory region.
#![cfg(feature = "koala")]
use std::io;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use memfd::{Memfd, MemfdOptions};
use memmap2::{MmapMut, MmapOptions};
use memmap_fixed::MmapFixed;
use thiserror::Error;

use crate::{ffi, ibv, rdmacm};

use interface::{AccessFlags, RemoteKey};
use interface::{AsHandle, Handle};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
}

pub struct MemoryRegion {
    mr: *mut ffi::ibv_mr,
    // mmap: MmapMut,
    mmap: MmapFixed,
    memfd: Memfd,
    file_off: usize,
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

impl AsHandle for MemoryRegion {
    #[inline]
    fn as_handle(&self) -> Handle {
        assert!(!self.mr.is_null());
        Handle(unsafe { &*self.mr }.handle)
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

        // let mut mmap = unsafe { MmapOptions::new().map_mut(memfd.as_file()) }?;
        // assert!(mmap.as_ptr() as usize % page_size() == 0);
        // let (mmap, file_off) = MmapAligned::map_aligned(memfd.as_file())?;
        let align = nbytes
            .checked_next_power_of_two()
            .expect("next_power_of_two: {len}");
        let layout = Layout::from_size_align(nbytes, align).unwrap();
        let target_addr = ADDRESS_MEDIATOR.allocate(layout);
        let mmap = MmapFixed::new(target_addr, nbytes, 0, memfd.as_file())?;
        let file_off = 0;

        let mr = unsafe {
            ffi::ibv_reg_mr(
                pd.pd,
                mmap.as_mut_ptr() as *mut _,
                nbytes as _,
                ibv::AccessFlags::from(access).0 .0 as _,
            )
        };

        if mr.is_null() {
            panic!("{:?}", Error::Io(io::Error::last_os_error()));
            // TODO(cjr): remove this panic after testing is finished
            Err(Error::Io(io::Error::last_os_error()))
        } else {
            Ok(Self {
                mr,
                mmap,
                memfd,
                file_off,
            })
        }
    }

    #[inline]
    pub fn memfd(&self) -> &Memfd {
        &self.memfd
    }

    #[inline]
    pub fn mr(&self) -> *mut ffi::ibv_mr {
        self.mr
    }

    #[inline]
    pub fn pd<'a>(&self) -> &ibv::ProtectionDomain<'a> {
        assert!(!self.mr.is_null());
        unsafe { (&(&*self.mr).pd).as_ref() }
    }

    #[inline]
    pub fn rkey(&self) -> RemoteKey {
        assert!(!self.mr.is_null());
        let mr = unsafe { &*self.mr };
        RemoteKey {
            rkey: mr.rkey,
            addr: mr.addr as u64,
        }
    }

    #[inline]
    pub fn file_off(&self) -> usize {
        self.file_off
    }
}

impl<'a, A> From<A> for rdmacm::MemoryRegion<'a>
where
    A: AsRef<MemoryRegion> + 'a,
{
    fn from(a: A) -> rdmacm::MemoryRegion<'a> {
        rdmacm::MemoryRegion(a.as_ref().mr, PhantomData)
    }
}

use lazy_static::lazy_static;
use std::alloc::Layout;
struct AddressMediator {
    current: spin::Mutex<usize>,
}

impl AddressMediator {
    const STARTING_ADDRESS: usize = 0x600000000000;

    fn new() -> Self {
        Self {
            current: spin::Mutex::new(Self::STARTING_ADDRESS),
        }
    }

    fn allocate(&self, layout: Layout) -> usize {
        let mut current = self.current.lock();
        let next = current.next_multiple_of(layout.align());
        *current = next + layout.size();
        next
    }
}

lazy_static! {
    static ref ADDRESS_MEDIATOR: AddressMediator = AddressMediator::new();
}
