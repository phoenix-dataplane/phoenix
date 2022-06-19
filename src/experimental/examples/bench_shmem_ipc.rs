use std::fs;
use std::fs::File;
use std::mem;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::slice;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, ForkResult};
use structopt::StructOpt;

use shmem_ipc::sharedring::{Receiver, Sender};

use experimental::ipc::{recv_fd, send_fd};
use experimental::*;

type Value = [u8; 64];

const SOCK_PATH: &str = "/tmp/bench_shmem_ipc.sock";

fn send_loop(sender: &mut Sender<Value>, num_iters: usize) -> anyhow::Result<()> {
    let mut scnt = 0; // the send count
    let mut value = [0u8; mem::size_of::<Value>()];
    while scnt < num_iters {
        unsafe {
            sender.sender_mut().send(|ptr, count| -> usize {
                let buffer = slice::from_raw_parts_mut(ptr, count);
                let tmp = scnt;
                for v in buffer {
                    if scnt >= num_iters {
                        break;
                    }
                    *v = value;
                    value[scnt % mem::size_of::<Value>()] =
                        value[scnt % mem::size_of::<Value>()].wrapping_add(1);
                    scnt += 1;
                }
                scnt - tmp
            })?;
        }
    }
    Ok(())
}

fn recv_loop(receiver: &mut Receiver<Value>, num_iters: usize) -> anyhow::Result<()> {
    let mut rcnt = 0;
    let mut value = [0u8; mem::size_of::<Value>()];
    while rcnt < num_iters {
        unsafe {
            receiver.receiver_mut().recv(|ptr, count| -> usize {
                let buffer = slice::from_raw_parts(ptr, count);
                let tmp = rcnt;
                for v in buffer {
                    if rcnt >= num_iters {
                        break;
                    }
                    assert_eq!(*v, value);
                    value[rcnt % mem::size_of::<Value>()] =
                        value[rcnt % mem::size_of::<Value>()].wrapping_add(1);
                    rcnt += 1;
                }
                rcnt - tmp
            })?;
        }
    }
    Ok(())
}

fn run_sender(opts: &QueueOpt) -> anyhow::Result<()> {
    // 1. let the sender create the set of file descriptors
    let capacity = opts.bound * mem::size_of::<Value>();
    let mut sender = Sender::new(capacity)?;
    // 2. pass the file descriptors to the receiver
    let fds = vec![
        sender.memfd().as_raw_fd(),
        sender.empty_signal().as_raw_fd(),
        sender.full_signal().as_raw_fd(),
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
    send_loop(&mut sender, opts.warm_iters)?;
    // 5. the main loop
    send_loop(&mut sender, opts.total_iters)?;
    Ok(())
}

fn run_receiver(opts: &QueueOpt) -> anyhow::Result<()> {
    // 1. receive the file descriptors from the sender
    let sock = UnixDatagram::bind(SOCK_PATH)?;
    let fds = recv_fd(&sock)?;
    let (memfd, empty_signal, full_signal) = unsafe {
        (
            File::from_raw_fd(fds[0]),
            File::from_raw_fd(fds[1]),
            File::from_raw_fd(fds[2]),
        )
    };
    // 2. create the Receiver object from the set of file descriptors
    let capacity = opts.bound * mem::size_of::<Value>();
    let mut receiver = Receiver::open(capacity, memfd, empty_signal, full_signal)?;
    // 3. find a pair of hyperthreads and bind to a one of them
    let (_, receiver_core) = get_hyperthread_core_pair();
    set_affinity_for_current(receiver_core)?;
    // 4. warmup
    recv_loop(&mut receiver, opts.warm_iters)?;

    // 5. time it!
    let timer = Instant::now();
    recv_loop(&mut receiver, opts.total_iters)?;

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
