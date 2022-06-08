use crossbeam::channel;
use crossbeam::thread;
use std::time::Instant;
use structopt::StructOpt;

use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    thread::scope(|s| {
        let opts = &opts;
        let (tx1, rx1) = channel::bounded(opts.bound);
        let (tx2, rx2) = channel::bounded(opts.bound);
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            if opts.set_affinity {
                set_affinity_for_current(sender_core).unwrap();
            }
            for i in 0..opts.warm_iters + opts.total_iters {
                while let Err(_) = tx1.try_send(i) {}
                loop {
                    match rx2.try_recv() {
                        Ok(_x) => {
                            break;
                        }
                        Err(_) => {}
                    }
                }
            }
        });

        let receiver = s.spawn(move |_| {
            if opts.set_affinity {
                set_affinity_for_current(receiver_core).unwrap();
            }
            for i in 0..opts.warm_iters {
                loop {
                    match rx1.try_recv() {
                        Ok(x) => {
                            assert_eq!(i, x);
                            break;
                        }
                        Err(_) => {}
                    }
                }
                while let Err(_) = tx2.try_send(i) {}
            }

            let start = Instant::now();
            for i in 0..opts.total_iters {
                loop {
                    match rx1.try_recv() {
                        Ok(_x) => {
                            break;
                        }
                        Err(_) => {}
                    }
                }
                while let Err(_) = tx2.try_send(i) {}
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
