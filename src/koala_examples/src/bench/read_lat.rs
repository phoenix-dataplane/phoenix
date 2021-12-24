use std::time::Instant;

use crate::bench::util::{handshake, print_lat, Context};
use interface::SendFlags;
use libkoala::verbs::MemoryRegion;
use libkoala::{cm, verbs::WcStatus, Error};

pub fn run_client(ctx: &Context) -> Result<(), Error> {
    let send_flags = SendFlags::SIGNALED;

    let mut builder = cm::CmId::resolve_route((ctx.opt.ip.to_owned(), ctx.opt.port))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut read_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");
    let mut write_mr: MemoryRegion<u8> = pre_id
        .alloc_read(ctx.opt.size)
        .expect("Memory registration failed!");
    // unsafe_write_bytes!(u32, 0, write_mr.as_mut_slice());

    let (id, rkey) = handshake(pre_id, &ctx, &write_mr.rkey()).expect("Handshake failed!");
    eprintln!("Handshake finished");

    let mut times = Vec::with_capacity(ctx.opt.num + 1);
    for _i in 0..ctx.opt.num {
        times.push(Instant::now());

        unsafe {
            id.post_read(&mut read_mr, .., 0, send_flags, rkey, 0)
                .expect("Post read failed!");
        }
        let wc = id.get_send_comp().expect("Get send comp failed!");
        assert_eq!(wc.status, WcStatus::Success);
    }
    unsafe_write_bytes!(u32, ctx.opt.num as u32, write_mr.as_mut_slice());

    times.push(Instant::now());
    print_lat(ctx, &times);
    Ok(())
}

pub fn run_server(ctx: &Context) -> Result<(), Error> {
    let send_flags = SendFlags::SIGNALED;

    let listener = libkoala::cm::CmIdListener::bind((ctx.opt.ip.to_owned(), ctx.opt.port))
        .expect("Listener bind failed");
    eprintln!("listen_id created");

    let mut builder = listener.get_request().expect("Get request failed!");
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut read_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");
    let write_mr: MemoryRegion<u8> = pre_id
        .alloc_read(ctx.opt.size)
        .expect("Memory registration failed!");
    unsafe_write_bytes!(u32, u32::MAX, read_mr.as_mut_slice());

    let (id, rkey) = handshake(pre_id, &ctx, &write_mr.rkey()).expect("Handshake failed!");
    eprintln!("Handshake finished");

    while unsafe_read_volatile!(u32, read_mr.as_ptr() as *const u32) != ctx.opt.num as u32 {
        unsafe {
            id.post_read(&mut read_mr, .., 0, send_flags, rkey, 0)
                .expect("Post read failed!");
        }
        let wc = id.get_send_comp().expect("Get send comp failed!");
        assert_eq!(wc.status, WcStatus::Success);
    }

    Ok(())
}
