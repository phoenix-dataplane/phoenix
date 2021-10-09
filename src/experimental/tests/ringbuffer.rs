#[cfg(test)]
use crossbeam::thread;
use experimental::ringbuffer::RingBuffer;
use rand::prelude::*;
use rand::{rngs::StdRng, SeedableRng};

const NUM_ITERS: usize = 100_000;
const SEED: u64 = 999;

#[test]
fn ringbuffer_correctness() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Element([usize; 3]);

    let queue: RingBuffer<(usize, Element)> = RingBuffer::with_capacity(32);
    thread::scope(|s| {
        let sender = s.spawn(|_| {
            let mut rng = StdRng::seed_from_u64(SEED);
            for i in 0..NUM_ITERS {
                let arr = Element([rng.gen(), rng.gen(), rng.gen()]);
                queue.push((i, arr));
            }
        });
        let receiver = s.spawn(|_| {
            // send and receiver use the same seed
            let mut rng = StdRng::seed_from_u64(SEED);
            for i in 0..NUM_ITERS {
                let arr = Element([rng.gen(), rng.gen(), rng.gen()]);
                let (j, arr2) = queue.wait_and_pop();
                assert_eq!((i, arr), (j, arr2));
            }
        });

        sender.join().unwrap();
        receiver.join().unwrap();
    })
    .unwrap();
}

/// Test if server will crash when client is crashed.
#[test]
fn ringbuffer_fault_isolation1() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Element(usize, u8, i16, f64);

    let queue: RingBuffer<(usize, Element)> = RingBuffer::with_capacity(32);
    thread::scope(|s| {
        let sender = s.spawn(|_| {
            let mut rng = StdRng::seed_from_u64(SEED);
            for i in 0..NUM_ITERS {
                let e = Element(rng.gen(), rng.gen(), rng.gen(), rng.gen());
                queue.push((i, e));
            }
        });
        let receiver = s.spawn(|_| {
            let mut rng = StdRng::seed_from_u64(SEED);
            for i in 0..NUM_ITERS {
                let e = Element(rng.gen(), rng.gen(), rng.gen(), rng.gen());
                let (j, e2) = queue.wait_and_pop();
                assert_eq!((i, e), (j, e2));
            }
        });

        sender.join().unwrap();
        receiver.join().unwrap();
    })
    .unwrap();
}

/// Test if the crash of one app can affect other apps.
#[test]
fn ringbuffer_fault_isolation2() {}

/// Test if an malicious client can take down the server.
#[test]
fn ringbuffer_malicious_client() {}
