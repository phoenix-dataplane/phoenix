#![allow(dead_code)]
#![allow(unused_imports)]
#![feature(allocator_api)]
#![feature(unix_socket_ancillary_data)]
#![feature(nonnull_slice_from_raw_parts)]
use std::io;
use structopt::StructOpt;

#[macro_use]
extern crate log;

pub mod ringbuffer;
pub mod shm;
pub mod ipc;
mod aligned_alloc;
mod regmr;
mod shmalloc;

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "benchmark lockless queue")]
pub struct QueueOpt {
    /// Total number of iterations
    #[structopt(short, long, default_value = "10000000")]
    pub total_iters: usize,

    /// Number of warmup iterations
    #[structopt(short, long, default_value = "1000")]
    pub warm_iters: usize,

    /// Queue bound
    #[structopt(short, long, default_value = "1024")]
    pub bound: usize,
}

pub fn set_affinity_for_current(cpu_idx: usize) -> io::Result<usize> {
    unsafe {
        let nprocs = libc::sysconf(libc::_SC_NPROCESSORS_ONLN) as usize;
        let cpu_idx = cpu_idx % nprocs;
        let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(cpu_idx, &mut cpuset);
        // pid = 0 defaults to current thread
        let err = libc::sched_setaffinity(0, std::mem::size_of_val(&cpuset), &cpuset);
        if err != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cpu_idx)
    }
}

pub fn check_hyperthread_enabled() -> bool {
    const SYSFS_SMT_ACTIVE: &str = "/sys/devices/system/cpu/smt/active";
    std::fs::read(SYSFS_SMT_ACTIVE)
        .map(|content| content == vec![b'1', b'\n'])
        .unwrap_or_default()
}

pub fn get_hyperthread_core_pair() -> (usize, usize) {
    assert!(check_hyperthread_enabled());
    let ncpus = num_cpus::get();
    let left = 0;
    let right = (left + ncpus / 2) % ncpus;
    (left, right)
}
