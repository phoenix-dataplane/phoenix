use std::cmp::min;
use std::time::Instant;

use crate::bench::util::{print_lat, Context};
use interface::SendFlags;
use libkoala::verbs::MemoryRegion;
use libkoala::{cm, verbs::WcStatus, Error};

pub fn run_client(ctx: &Context) -> Result<(), Error> {
    let mut send_flags = SendFlags::empty();
    if ctx.cap.max_inline_data as usize >= ctx.opt.size {
        send_flags = send_flags | SendFlags::INLINE;
    }
    send_flags |= SendFlags::SIGNALED;

    let mut builder = cm::CmId::resolve_route((ctx.opt.ip.to_owned(), ctx.opt.port))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut recv_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");
    let send_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");

    for _i in 0..min(ctx.opt.num, ctx.cap.max_recv_wr as usize) {
        unsafe {
            pre_id
                .post_recv(&mut recv_mr, .., 0)
                .expect("Post recv failed!");
        }
    }
    let id = pre_id.connect(None).expect("Connect failed!");
    eprintln!("Connection established");

    let mut times = Vec::with_capacity(ctx.opt.num + 1);
    for i in 0..ctx.opt.num {
        times.push(Instant::now());

        id.post_send(&send_mr, .., 0, send_flags)
            .expect("Post send failed!");
        let wc = id.get_send_comp().expect("Get send comp failed!");
        assert_eq!(wc.status, WcStatus::Success);

        let wc = id.get_recv_comp().expect("Get recv comp failed!");
        assert_eq!(wc.status, WcStatus::Success);
        if i + (ctx.cap.max_recv_wr as usize) < ctx.opt.num {
            unsafe {
                id.post_recv(&mut recv_mr, .., 0)
                    .expect("Post recv failed!");
            }
        }
    }
    times.push(Instant::now());
    print_lat(ctx, &times);

    Ok(())
}

pub fn run_server(ctx: &Context) -> Result<(), Error> {
    let mut send_flags = SendFlags::empty();
    if ctx.cap.max_inline_data as usize >= ctx.opt.size {
        send_flags = send_flags | SendFlags::INLINE;
    }
    send_flags |= SendFlags::SIGNALED;

    let listener = libkoala::cm::CmIdListener::bind((ctx.opt.ip.to_owned(), ctx.opt.port))
        .expect("Listener bind failed");
    eprintln!("listen_id created");

    let mut builder = listener.get_request().expect("Get request failed!");
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");

    let mut recv_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");
    let send_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");

    for _i in 0..min(ctx.opt.num, ctx.cap.max_recv_wr as usize) {
        unsafe {
            pre_id
                .post_recv(&mut recv_mr, .., 0)
                .expect("Post recv failed!");
        }
    }

    let id = pre_id.accept(None).expect("Accept failed!");
    eprintln!("Connection established");

    for i in 0..ctx.opt.num {
        let wc = id.get_recv_comp().expect("Get recv comp failed!");
        assert_eq!(wc.status, WcStatus::Success);
        if i as u32 + ctx.cap.max_recv_wr < ctx.opt.num as u32 {
            unsafe {
                id.post_recv(&mut recv_mr, .., 0)
                    .expect("Post recv failed!");
            }
        }

        id.post_send(&send_mr, .., 0, send_flags)
            .expect("Post send failed!");
        let wc = id.get_send_comp().expect("Get send comp failed!");
        assert_eq!(wc.status, WcStatus::Success);
    }

    Ok(())
}
