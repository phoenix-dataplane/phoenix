use atomicring::AtomicRingQueue;
use crossbeam::thread;
use std::time::Instant;
use structopt::StructOpt;

use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    let ring = AtomicRingQueue::with_capacity(opts.bound);
    thread::scope(|s| {
        let ring = &ring;
        let opts = &opts;
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            let ring = &ring;
            set_affinity_for_current(sender_core).unwrap();
            for i in 0..opts.warm_iters + opts.total_iters {
                while let Err(_) = ring.try_push(i) {
                    std::thread::yield_now();
                }
            }
        });

        let receiver = s.spawn(move |_| {
            let ring = &ring;
            set_affinity_for_current(receiver_core).unwrap();
            for i in 0..opts.warm_iters {
                let x = ring.pop();
                assert_eq!(i, x);
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                let _x = ring.pop();
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
