use chashmap::CHashMap;
use dashmap::DashMap;
use fnv::FnvBuildHasher;
use std::{collections::HashMap, time::Instant};

fn print(tag: &str, dura: u128, num: u128) {
    println!(
        "{}, Duration: {}, num: {}, avg: {}",
        tag,
        dura,
        num,
        dura as f64 / num as f64
    );
}
fn main() {
    let num = 100000;

    let mut map0 = HashMap::<u32, u32, FnvBuildHasher>::default();
    let start = Instant::now();
    for i in 0..num {
        let key = i as u32;
        let value = i as u32;
        map0.insert(key, value);
    }
    let dura = start.elapsed().as_nanos();
    print("HashMap insert", dura, num);

    let start = Instant::now();
    for i in 0..num {
        let _value = map0.get(&(i as u32)).unwrap();
    }
    let dura = start.elapsed().as_nanos();
    print("HashMap get", dura, num);

    let map1 = DashMap::<u32, u32, FnvBuildHasher>::default();
    let start = Instant::now();
    for i in 0..num {
        let key = i as u32;
        let value = i as u32;
        map1.insert(key, value);
    }
    let dura = start.elapsed().as_nanos();
    print("DashMap insert", dura, num);

    let start = Instant::now();
    for i in 0..num {
        let _value = map1.get(&(i as u32)).unwrap();
    }
    let dura = start.elapsed().as_nanos();
    print("DashMap get", dura, num);

    let map2 = CHashMap::<u32, u32>::default();
    let start = Instant::now();
    for i in 0..num {
        let key = i as u32;
        let value = i as u32;
        map2.insert(key, value);
    }
    let dura = start.elapsed().as_nanos();
    print("CHashMap insert", dura, num);

    let start = Instant::now();
    for i in 0..num {
        let _value = map2.get(&(i as u32)).unwrap();
    }
    let dura = start.elapsed().as_nanos();
    print("CHashMap get", dura, num);
}
