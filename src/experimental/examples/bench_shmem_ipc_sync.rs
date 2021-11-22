use std::fs;
use std::fs::File;
use std::mem;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, ForkResult};
use structopt::StructOpt;

use shmem_ipc::sharedring::{Receiver, Sender};

use experimental::*;
use ipc::{recv_fd, send_fd};

type Value = [u8; 64];

const SOCK_PATH: &str = "/tmp/bench_shmem_ipc.sock";

fn send_loop(
    wq: &mut Sender<Value>,
    cq: &mut Receiver<Value>,
    num_iters: usize,
) -> anyhow::Result<()> {
    let mut cnt = 0; // the recv count
    let mut value = [0u8; mem::size_of::<Value>()];
    while cnt < num_iters {
        let mut sent = false;
        while !sent {
            wq.sender_mut().send(|ptr, _count| {
                unsafe {
                    ptr.write(value);
                }
                value[cnt % mem::size_of::<Value>()] =
                    value[cnt % mem::size_of::<Value>()].wrapping_add(1);
                sent = true;
                1
            })?;
        }

        let mut received = false;
        while !received {
            cq.receiver_mut().recv(|ptr, _count| {
                let v = unsafe { ptr.read() };
                assert_eq!(v, value);
                received = true;
                1
            })?;
        }

        cnt += 1;
    }
    Ok(())
}

fn recv_loop(
    wq: &mut Receiver<Value>,
    cq: &mut Sender<Value>,
    num_iters: usize,
) -> anyhow::Result<()> {
    let mut cnt = 0;
    let mut value = [0u8; mem::size_of::<Value>()];
    while cnt < num_iters {
        let mut received = false;
        while !received {
            wq.receiver_mut().recv(|ptr, _count| {
                let v = unsafe { ptr.read() };
                assert_eq!(v, value);
                value[cnt % mem::size_of::<Value>()] =
                    value[cnt % mem::size_of::<Value>()].wrapping_add(1);
                received = true;
                1
            })?;
        }

        let mut sent = false;
        while !sent {
            cq.sender_mut().send(|ptr, _count| {
                unsafe {
                    ptr.write(value);
                }
                sent = true;
                1
            })?;
        }
        cnt += 1;
    }
    Ok(())
}

fn run_sender(opts: &QueueOpt) -> anyhow::Result<()> {
    // 1. let the sender create the set of file descriptors
    let capacity = opts.bound * mem::size_of::<Value>();
    let mut wq = Sender::new(capacity)?;
    let mut cq = Receiver::new(capacity)?;
    // 2. pass the file descriptors to the receiver
    let fds = vec![
        wq.memfd().as_raw_fd(),
        wq.empty_signal().as_raw_fd(),
        wq.full_signal().as_raw_fd(),
        cq.memfd().as_raw_fd(),
        cq.empty_signal().as_raw_fd(),
        cq.full_signal().as_raw_fd(),
    ];
    let sock = UnixDatagram::unbound()?;
    let mut retries = 0;
    while retries < 5 {
        match send_fd(&sock, SOCK_PATH, &fds) {
            Ok(()) => break,
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
        retries += 1;
    }
    // 3. find a pair of hyperthreads and bind to a one of them
    let (sender_core, _) = get_hyperthread_core_pair();
    set_affinity_for_current(sender_core)?;
    // 4. warmup
    send_loop(&mut wq, &mut cq, opts.warm_iters)?;
    // 5. the main loop
    send_loop(&mut wq, &mut cq, opts.total_iters)?;
    Ok(())
}

fn run_receiver(opts: &QueueOpt) -> anyhow::Result<()> {
    // 1. receive the file descriptors from the sender
    let sock = UnixDatagram::bind(SOCK_PATH)?;
    let fds = recv_fd(&sock)?;
    let (wq_memfd, wq_empty_signal, wq_full_signal, cq_memfd, cq_empty_signal, cq_full_signal) = unsafe {
        (
            File::from_raw_fd(fds[0]),
            File::from_raw_fd(fds[1]),
            File::from_raw_fd(fds[2]),
            File::from_raw_fd(fds[3]),
            File::from_raw_fd(fds[4]),
            File::from_raw_fd(fds[5]),
        )
    };
    // 2. create the Receiver object from the set of file descriptors
    let capacity = opts.bound * mem::size_of::<Value>();
    let mut wq = Receiver::open(capacity, wq_memfd, wq_empty_signal, wq_full_signal)?;
    let mut cq = Sender::open(capacity, cq_memfd, cq_empty_signal, cq_full_signal)?;
    // 3. find a pair of hyperthreads and bind to a one of them
    let (_, receiver_core) = get_hyperthread_core_pair();
    set_affinity_for_current(receiver_core)?;
    // 4. warmup
    recv_loop(&mut wq, &mut cq, opts.warm_iters)?;

    // 5. time it!
    let timer = Instant::now();
    recv_loop(&mut wq, &mut cq, opts.total_iters)?;

    let dura = timer.elapsed().as_secs_f64();
    println!(
        "{}: {} Mop/s in {} iters, {:.0} ns/op",
        file!(),
        opts.total_iters as f64 / dura / 1e6,
        opts.total_iters,
        1e9 * dura / opts.total_iters as f64,
    );
    Ok(())
}

fn main() {
    let opts = QueueOpt::from_args();

    if Path::new(SOCK_PATH).exists() {
        fs::remove_file(SOCK_PATH).unwrap();
    }

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            run_sender(&opts).unwrap();
            // waitpid
            let status = waitpid(child, None).unwrap();
            assert!(matches!(status, WaitStatus::Exited(..)));
        }
        Ok(ForkResult::Child) => {
            run_receiver(&opts).unwrap();
        }
        Err(e) => panic!("fork failed: {}", e),
    }
}
