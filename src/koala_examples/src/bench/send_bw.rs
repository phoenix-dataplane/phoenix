use std::cmp::min;
use std::time::Instant;

use crate::bench::util::{print_bw, Context};
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

    let send_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(ctx.opt.size)
        .expect("Memory registration failed!");

    let id = pre_id.connect(None).expect("Connect failed!");
    eprintln!("Connection established");

    let mut tposted = Vec::with_capacity(ctx.opt.num);
    let mut tcompleted = Vec::with_capacity(ctx.opt.num);
    let mut wcs = Vec::with_capacity(CTX_POLL_BATCH);
    let mut scnt = 0;
    let mut ccnt = 0;
    let cq = &id.qp().send_cq;
    while scnt < ctx.opt.num || ccnt < ctx.opt.num {
        if scnt < ctx.opt.num && scnt - ccnt < ctx.cap.max_send_wr as usize {
            tposted.push(Instant::now());
            id.post_send(&send_mr, .., 0, send_flags)
                .expect("Post send failed!");
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

    print_bw(ctx, &tposted, &tcompleted);

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

    let mut rcnt = 0;
    let mut ccnt = 0;
    for _i in 0..min(ctx.opt.num, ctx.cap.max_recv_wr as usize) {
        unsafe {
            pre_id
                .post_recv(&mut recv_mr, .., 0)
                .expect("Post recv failed!");
        }
        rcnt += 1;
    }

    let id = pre_id.accept(None).expect("Accept failed!");
    eprintln!("Connection established");

    let mut wcs = Vec::with_capacity(CTX_POLL_BATCH);
    let cq = &id.qp().recv_cq;
    while rcnt < ctx.opt.num || ccnt < ctx.opt.num {
        if rcnt < ctx.opt.num && rcnt - ccnt < ctx.cap.max_recv_wr as usize {
            unsafe {
                id.post_recv(&mut recv_mr, .., 0)
                    .expect("Post recv failed!");
            }
            rcnt += 1;
        }
        if ccnt < ctx.opt.num {
            cq.poll_cq(&mut wcs).expect("Poll cq failed!");
            if !wcs.is_empty() {
                for wc in &wcs {
                    assert_eq!(wc.status, WcStatus::Success);
                    ccnt += 1;
                }
            }
        }
    }

    Ok(())
}
