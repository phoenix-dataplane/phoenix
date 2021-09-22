use crossbeam::thread;
use std::sync::mpsc;
use std::time::Instant;
use structopt::StructOpt;

use experimental::*;

fn main() {
    let opts = QueueOpt::from_args();

    thread::scope(|s| {
        let opts = &opts;
        let (tx, rx) = mpsc::sync_channel(opts.bound);
        let (sender_core, receiver_core) = get_hyperthread_core_pair();

        let sender = s.spawn(move |_| {
            set_affinity_for_current(sender_core).unwrap();
            for i in 0..opts.warm_iters + opts.total_iters {
                tx.send(i).unwrap();
            }
        });

        let receiver = s.spawn(move |_| {
            set_affinity_for_current(receiver_core).unwrap();
            for i in 0..opts.warm_iters {
                let x = rx.recv().unwrap();
                assert_eq!(i, x);
            }

            let start = Instant::now();
            for _i in 0..opts.total_iters {
                let _x = rx.recv().unwrap();
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