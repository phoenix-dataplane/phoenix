use crossbeam::thread;
use std::time::Instant;
use structopt::StructOpt;

use experimental::ringbuffer::RingBuffer;
use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    let wq = RingBuffer::with_capacity(opts.bound);
    let cq = RingBuffer::with_capacity(opts.bound);
    thread::scope(|s| {
        let wq = &wq;
        let cq = &cq;
        let opts = &opts;
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            set_affinity_for_current(sender_core).unwrap();
            let wq = &wq;
            let cq = &cq;
            for i in 0..opts.warm_iters + opts.total_iters {
                wq.push(i);
                let j = cq.wait_and_pop();
                assert_eq!(i, j);
            }
        });

        let receiver = s.spawn(move |_| {
            set_affinity_for_current(receiver_core).unwrap();
            let wq = &wq;
            let cq = &cq;
            for i in 0..opts.warm_iters {
                let x = wq.wait_and_pop();
                cq.push(x);
                assert_eq!(i, x);
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                let x = wq.wait_and_pop();
                cq.push(x);
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
