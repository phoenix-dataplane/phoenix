use std::cmp::min;
use std::time::Instant;

use crate::bench::util::{handshake, print_bw, Context};
use interface::SendFlags;
use libkoala::verbs::MemoryRegion;
use libkoala::{cm, verbs::WcStatus, Error};

const CTX_POLL_BATCH: usize = 16;

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

    let mut read_mr: MemoryRegion<u8> = pre_id
        .alloc_write(ctx.opt.size)
        .expect("Memory registration failed!");
    let mut write_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");
    unsafe_write_bytes!(u32, u32::MAX, read_mr.as_mut_slice());

    let (id, rkey) = handshake(pre_id, &ctx, &read_mr.rkey()).expect("Handshake failed!");
    eprintln!("Handshake finished");

    let mut tposted = Vec::with_capacity(ctx.opt.num);
    let mut tcompleted = Vec::with_capacity(ctx.opt.num);
    let mut wcs = Vec::with_capacity(CTX_POLL_BATCH);
    let mut scnt = 0;
    let mut ccnt = 0;
    let cq = &id.qp().send_cq;
    while scnt < ctx.opt.num || ccnt < ctx.opt.num {
        if scnt < ctx.opt.num && scnt - ccnt < ctx.cap.max_send_wr as usize {
            tposted.push(Instant::now());
            unsafe_write_bytes!(u32, scnt as u32, write_mr.as_mut_slice());
            id.post_write(&write_mr, .., 0, send_flags, rkey, 0)
                .expect("Post write failed!");
            scnt += 1;
        }
        if ccnt < ctx.opt.num {
            cq.poll_cq(&mut wcs).expect("Poll cq failed!");
            if !wcs.is_empty() {
                for wc in &wcs {
                    assert_eq!(wc.status, WcStatus::Success);
                    ccnt += 1;
                    tcompleted.push(Instant::now());
                }
            }
        }
    }
    
    unsafe_write_bytes!(u32, ctx.opt.num as u32, write_mr.as_mut_slice());
    id.post_write(&write_mr, .., 0, send_flags, rkey, 0)
        .expect("Post write failed!");
    let wc = id.get_send_comp().expect("Get write comp failed!");
    assert_eq!(wc.status, WcStatus::Success);

    print_bw(ctx, &tposted, &tcompleted);

    Ok(())
}

pub fn run_server(ctx: &Context) -> Result<(), Error> {
    let listener = libkoala::cm::CmIdListener::bind((ctx.opt.ip.to_owned(), ctx.opt.port))
        .expect("Listener bind failed");
    eprintln!("listen_id created");

    let mut builder = listener.get_request().expect("Get request failed!");
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut read_mr: MemoryRegion<u8> = pre_id
        .alloc_write(ctx.opt.size)
        .expect("Memory registration failed!");
    unsafe_write_bytes!(u32, u32::MAX, read_mr.as_mut_slice());

    let (_id, _rkey) = handshake(pre_id, &ctx, &read_mr.rkey()).expect("Handshake failed!");
    eprintln!("Handshake finished");

    while unsafe_read_volatile!(u32, read_mr.as_ptr() as *const u32) != ctx.opt.num as u32 {}

    Ok(())
}
