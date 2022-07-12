use masstree_analytics::mt_index::{MtIndex, ThreadInfo, threadinfo_purpose};

fn main() {
    println!("masstree_analytics");

    // Create the Masstree using the main thread and insert keys
    let ti = ThreadInfo::new(threadinfo_purpose::TI_MAIN, -1);
    let _mti = MtIndex::new(ti);

    // for i in 0..1000 {
    //     let key = i;
    //     let value = i;
    //     mti.put(key, value, ti);
    // }
}
