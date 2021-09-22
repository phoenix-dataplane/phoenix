use crossbeam::thread;
use crossbeam::utils::Backoff;
use ringbuf::RingBuffer;
use std::time::Instant;
use structopt::StructOpt;

use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    thread::scope(|s| {
        let opts = &opts;
        let rb = RingBuffer::<usize>::new(opts.bound);
        let (mut prod, mut cons) = rb.split();
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            set_affinity_for_current(sender_core).unwrap();
            let backoff = Backoff::new();
            for i in 0..opts.warm_iters + opts.total_iters {
                while let Err(_) = prod.push(i) {
                    // std::thread::yield_now();
                    backoff.snooze();
                }
            }
        });

        let receiver = s.spawn(move |_| {
            set_affinity_for_current(receiver_core).unwrap();
            let backoff = Backoff::new();
            for i in 0..opts.warm_iters {
                loop {
                    match cons.pop() {
                        Some(x) => {
                            assert_eq!(i, x);
                            break;
                        }
                        None => {
                            // std::thread::yield_now();
                            backoff.snooze();
                        }
                    }
                }
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                loop {
                    match cons.pop() {
                        Some(_x) => {
                            break;
                        }
                        None => {
                            // std::thread::yield_now();
                            backoff.snooze();
                        }
                    }
                }
            }

            println!(
                "{}: {} Mop/s in {} iters",
                file!(),
                opts.total_iters as f64 / start.elapsed().as_secs_f64() / 1e6,
                opts.total_iters,
            );
        });

        sender.join().unwrap();
        receiver.join().unwrap();
    })
    .unwrap();
}