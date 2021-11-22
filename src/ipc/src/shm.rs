//! Shared memory object.
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::Deref;
use std::sync::Arc;

use atomic_traits::Atomic;
use memfd::{Memfd, MemfdOptions};
use memmap2::{MmapOptions, MmapRaw};
use thiserror::Error;
use uuid::Uuid;

fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

fn round_to_page_size<T>() -> usize {
    let bytes = size_of::<T>();
    let ps = page_size();
    (bytes + ps - 1) / ps * ps
}

struct Inner {
    mmap: MmapRaw,
    memfd: Memfd,
}

pub struct ShmObject<T> {
    inner: Arc<Inner>,
    _marker: PhantomData<T>,
}

unsafe impl<T: Sync + Send + Atomic> Send for ShmObject<T> {}
unsafe impl<T: Sync + Send + Atomic> Sync for ShmObject<T> {}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
    #[error("ptr is null.")]
    PtrIsNull,
    #[error("Buffer too small.")]
    BufTooSmall,
}

impl Inner {
    fn new(nbytes: usize) -> Result<Self, Error> {
        let opts = MemfdOptions::default()
            .allow_sealing(true)
            .close_on_exec(false);
        let name = format!("shmobj-{}", Uuid::new_v4());
        let memfd = opts.create(name)?;
        memfd.as_file().set_len(nbytes as u64)?;

        let mmap = MmapOptions::new().map_raw(memfd.as_file())?;
        Ok(Inner { mmap, memfd })
    }

    fn open(nbytes: usize, file: File) -> Result<Self, Error> {
        let memfd = Memfd::try_from_file(file).map_err(|_| io::Error::last_os_error())?;
        let mmap = MmapOptions::new().map_raw(memfd.as_file())?;
        if mmap.len() < nbytes {
            return Err(Error::BufTooSmall);
        };
        Ok(Inner { mmap, memfd })
    }
}

impl<T> ShmObject<T>
where
    T: Sync + Send + Atomic,
{
    pub fn new(data: T) -> Result<Self, Error> {
        let nbytes = round_to_page_size::<T>();
        let inner = Inner::new(nbytes)?;
        unsafe { inner.mmap.as_mut_ptr().cast::<T>().write(data) };
        Ok(ShmObject {
            inner: Arc::new(inner),
            _marker: PhantomData,
        })
    }

    pub fn open(file: File) -> Result<Self, Error> {
        let nbytes = round_to_page_size::<T>();
        let inner = Inner::open(nbytes, file)?;
        Ok(ShmObject {
            inner: Arc::new(inner),
            _marker: PhantomData,
        })
    }

    pub fn memfd(this: &ShmObject<T>) -> &Memfd {
        &this.inner.memfd
    }

    pub fn as_ptr(this: &ShmObject<T>) -> *const T {
        this.inner.mmap.as_ptr() as *const T
    }
}

impl<T> Clone for ShmObject<T>
where
    T: Sync + Send + Atomic,
{
    fn clone(&self) -> Self {
        ShmObject {
            inner: Arc::clone(&self.inner),
            _marker: PhantomData,
        }
    }
}

impl<T> Deref for ShmObject<T>
where
    T: Sync + Send + Atomic,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        unsafe { &*ShmObject::as_ptr(self) }
    }
}
