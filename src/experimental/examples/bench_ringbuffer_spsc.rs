use crossbeam::thread;
use std::time::Instant;
use structopt::StructOpt;

use experimental::ringbuffer::RingBuffer;
use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    let ring = RingBuffer::with_capacity(opts.bound);
    thread::scope(|s| {
        let ring = &ring;
        let opts = &opts;
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            let ring = &ring;
            set_affinity_for_current(sender_core).unwrap();
            for i in 0..opts.warm_iters + opts.total_iters {
                ring.push(i);
            }
        });

        let receiver = s.spawn(move |_| {
            let ring = &ring;
            set_affinity_for_current(receiver_core).unwrap();
            for i in 0..opts.warm_iters {
                let x = ring.wait_and_pop();
                assert_eq!(i, x);
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                let _x = ring.wait_and_pop();
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
