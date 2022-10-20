#![feature(scoped_threads)]
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};
use std::thread;
use std::time::{Duration, Instant};

use chashmap::CHashMap;
use cht::map::HashMap as ChtHashMap;
use dashmap::DashMap;
// use flurry::{HashMap as FlurryHashMap, HashMapRef as FlurryHashMapRef};
use contrie::ConMap;
use fnv::FnvBuildHasher;
use sharded::Map as ShardedMap;

use sharded_slab::Slab;

// Concurrent key-value adapter trait.
trait KvAdapter<K, V> {
    fn insert_kv(&self, key: K, val: V);
    fn get_kv(&self, key: &K);
}

impl<K: Eq + Hash, V> KvAdapter<K, V> for DashMap<K, V, FnvBuildHasher> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let _val = self.get(key);
    }
}

impl<K: Eq + Hash, V> KvAdapter<K, V> for spin::Mutex<HashMap<K, V, FnvBuildHasher>> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.lock().insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let _val = self.lock().get(key);
    }
}

impl<K: Eq + Hash, V> KvAdapter<K, V> for CHashMap<K, V> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let _val = self.get(key);
    }
}

impl<K: Eq + Hash, V: Clone> KvAdapter<K, V> for ChtHashMap<K, V, FnvBuildHasher> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let _val = self.get(key);
    }
}

impl<K: Eq + Hash, V: Clone> KvAdapter<K, V> for ShardedMap<K, V> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let (key, shard) = self.read(key);
        let _val = shard.get(key);
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> KvAdapter<K, V> for ConMap<K, V, S> {
    #[inline]
    fn insert_kv(&self, key: K, val: V) {
        self.insert(key, val);
    }
    #[inline]
    fn get_kv(&self, key: &K) {
        let _val = self.get(key);
    }
}

impl<V> KvAdapter<u32, V> for Slab<V> {
    #[inline]
    fn insert_kv(&self, _key: u32, val: V) {
        self.insert(val);
    }
    #[inline]
    fn get_kv(&self, key: &u32) {
        let _val = self.get(*key as usize);
    }
}

fn print(desc: &str, dura: Duration, num: usize) {
    println!(
        "{}, duration: {:?}, num: {}, latency: {} ns/ops, tput: {} Mops/s",
        desc,
        dura,
        num,
        dura.as_nanos() as usize / num,
        num as f64 * 1e-6 / dura.as_secs_f64(),
    );
}

fn bench<Map: KvAdapter<u32, u32> + Sync + 'static>(nthreads: usize, name: &'static str, map: Map) {
    let num = 1000000;
    let bound = num as u32;

    // bench insert
    let start = Instant::now();

    thread::scope(|s| {
        for _ in 0..nthreads {
            s.spawn(|| {
                let start = Instant::now();
                for i in 0..num {
                    let key = fastrand::u32(..bound);
                    let value = i as u32;
                    map.insert_kv(key, value);
                }
                let dura = start.elapsed();
                print(&format!("{} insert", name), dura, num);
            });
        }
    });

    // print insert stats
    let dura = start.elapsed();
    print(
        &format!(
            "Aggregated {} insert statistics across {} threads",
            name, nthreads
        ),
        dura,
        num * nthreads,
    );

    // bench get
    let start = Instant::now();

    thread::scope(|s| {
        for _ in 0..nthreads {
            s.spawn(|| {
                let start = Instant::now();
                for i in 0..num {
                    let _value = map.get_kv(&(i as u32));
                }
                let dura = start.elapsed();
                print(&format!("{} get", name), dura, num);
            });
        }
    });

    // print get stats
    let dura = start.elapsed();
    print(
        &format!(
            "Aggregated {} get statistics across {} threads",
            name, nthreads
        ),
        dura,
        num * nthreads,
    );
}

fn bench_concurrent(max_concurrency: usize) {
    for nthreads in 1..=max_concurrency {
        println!("\nTesting across {} threads...", nthreads);

        let slab = Slab::new();
        bench(nthreads, "sharded_slab", slab);

        let map = DashMap::<u32, u32, FnvBuildHasher>::default();
        bench(nthreads, "DashMap", map);

        let map = spin::Mutex::new(HashMap::<u32, u32, FnvBuildHasher>::default());
        bench(nthreads, "spin::Mutex<HashMap>", map);

        let map = ChtHashMap::<u32, u32, FnvBuildHasher>::default();
        bench(nthreads, "cht::HashMap", map);

        let map = CHashMap::<u32, u32>::default();
        bench(nthreads, "CHashMap", map);

        let map = ShardedMap::<u32, u32>::default();
        bench(nthreads, "sharded::Map", map);

        // let map = FlurryHashMap::<u32, u32, FnvBuildHasher>::default();
        // // let map = map.pin();
        // bench(nthreads, "flurry::HashMapRef", map);

        let map = ConMap::<u32, u32, FnvBuildHasher>::with_hasher(Default::default());
        bench(nthreads, "contrie::ConMap", map);
    }
}

fn main() {
    let num = 1000000;
    let bound = num as u32;

    let mut map0 = HashMap::<u32, u32, FnvBuildHasher>::default();
    let start = Instant::now();
    for i in 0..num {
        let key = fastrand::u32(..bound);
        let value = i as u32;
        map0.insert(key, value);
    }
    let dura = start.elapsed();
    print("baseline HashMap insert", dura, num);

    let start = Instant::now();
    for i in 0..num {
        let _value = map0.get(&(i as u32));
    }
    let dura = start.elapsed();
    print("baseline HashMap get", dura, num);

    bench_concurrent(8);
    return;
}
