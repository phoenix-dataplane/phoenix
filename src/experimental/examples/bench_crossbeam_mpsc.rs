use crossbeam::thread;
use crossbeam::channel;
use crossbeam::utils::Backoff;
use std::time::Instant;
use structopt::StructOpt;

use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    thread::scope(|s| {
        let opts = &opts;
        let (tx, rx) = channel::bounded(opts.bound);
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            set_affinity_for_current(sender_core).unwrap();
            let backoff = Backoff::new();
            for i in 0..opts.warm_iters + opts.total_iters {
                // tx.send(i).unwrap();
                while let Err(_) = tx.try_send(i) {
                    backoff.snooze();
                }
            }
        });

        let receiver = s.spawn(move |_| {
            set_affinity_for_current(receiver_core).unwrap();
            let backoff = Backoff::new();
            for i in 0..opts.warm_iters {
                // let x = rx.recv().unwrap();
                // assert_eq!(i, x);
                loop {
                    match rx.try_recv() {
                        Ok(x) => {
                            assert_eq!(i, x);
                            break;
                        }
                        Err(_) => {
                            // std::thread::yield_now();
                            backoff.snooze();
                        }
                    }
                }
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                // let _x = rx.recv().unwrap();
                loop {
                    match rx.try_recv() {
                        Ok(_x) => {
                            break;
                        }
                        Err(_) => {
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