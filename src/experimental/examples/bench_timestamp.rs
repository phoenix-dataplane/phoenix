use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    let n = 1000000;
    let start = Instant::now();
    for i in 0..n {
        let now = Instant::now();
    }
    let finish = Instant::now();
    let sum = finish.duration_since(start).as_nanos() as f64;
    println!("{} {}", sum, 1.0 * sum / n as f64);
}
