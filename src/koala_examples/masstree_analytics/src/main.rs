extern crate link_cplusplus;

// NOTE(cjr): Need to link two libraries here. This is VERY tricky!
#[link(name = "mt_index", kind = "static")]
#[link(name = "masstree", kind = "static")]
extern "C" {
    // this is rustified prototype of the function from our C library
    fn mt_index_create() -> *mut ();
    fn mt_index_destroy(obj: *mut ());
    fn mt_index_setup(obj: *mut (), ti: *mut ());
    fn mt_index_swap_endian(x: &mut u64);
    fn mt_index_put(obj: *mut (), key: usize, value: usize, ti: *mut ());
    fn mt_index_get(obj: *mut (), key: usize, value: &mut usize, ti: *mut ()) -> bool;
    fn mt_index_sum_in_range(obj: *mut (), cur_key: usize, range: usize, ti: *mut ()) -> usize;
}

fn main() {
    println!("Hello, world from Rust!");

    // calling the function from foo library
    unsafe {
        let _mt_index = mt_index_create();
    };
}
