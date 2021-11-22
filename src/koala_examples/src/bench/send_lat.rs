use std::time::Instant;

use crate::bench::util::{poll_cq_and_check, print_lat, Context};
use interface::SendFlags;
use libkoala::{cm, Error};

pub fn run_client(ctx: &Context) -> Result<(), Error> {
    let mut send_flags = SendFlags::empty();
    if ctx.attr.cap.max_inline_data as usize >= ctx.opt.size {
        send_flags = send_flags | SendFlags::INLINE;
    }

    let mut recv_msg = vec![0; ctx.opt.size];
    let send_msg = vec![0; ctx.opt.size];

    let id = cm::CmId::create_ep(&ctx.ai, None, Some(&ctx.attr))?;
    let recv_mr = id.reg_msgs(&recv_msg)?;
    let send_mr = id.reg_msgs(&send_msg)?;

    for _i in 0..ctx.opt.num {
        unsafe {
            id.post_recv(0, &mut recv_msg, &recv_mr)?;
        }
    }
    id.connect(None)?;

    let mut times = Vec::new();
    let mut wcs = Vec::with_capacity(1);
    let recv_cq = &id.qp.as_ref().unwrap().recv_cq;
    let send_cq = &id.qp.as_ref().unwrap().send_cq;
    for i in 0..ctx.opt.num {
        times.push(Instant::now());
        if i == ctx.opt.num - 1 {
            send_flags |= SendFlags::SIGNALED;
        }
        id.post_send(0, &send_msg, &send_mr, send_flags)?;
        poll_cq_and_check(recv_cq, &mut wcs)?;
    }
    poll_cq_and_check(send_cq, &mut wcs)?;
    times.push(Instant::now());

    print_lat(ctx, times);

    Ok(())
}

pub fn run_server(ctx: &Context) -> Result<(), Error> {
    let mut send_flags = SendFlags::empty();
    if ctx.attr.cap.max_inline_data as usize >= ctx.opt.size {
        send_flags = send_flags | SendFlags::INLINE;
    }

    let mut recv_msg = vec![0; ctx.opt.size];
    let send_msg = vec![0; ctx.opt.size];

    let listen_id = cm::CmId::create_ep(&ctx.ai, None, Some(&ctx.attr))?;
    listen_id.listen(1)?;
    let id = listen_id.get_request()?;
    let recv_mr = id.reg_msgs(&recv_msg)?;
    let send_mr = id.reg_msgs(&send_msg)?;

    for _i in 0..ctx.opt.num {
        unsafe {
            id.post_recv(0, &mut recv_msg, &recv_mr)?;
        }
    }
    id.accept(None)?;

    let mut wcs = Vec::with_capacity(1);
    let recv_cq = &id.qp.as_ref().unwrap().recv_cq;
    let send_cq = &id.qp.as_ref().unwrap().send_cq;
    for i in 0..ctx.opt.num {
        poll_cq_and_check(recv_cq, &mut wcs)?;
        if i == ctx.opt.num - 1 {
            send_flags |= SendFlags::SIGNALED;
        }
        id.post_send(0, &send_msg, &send_mr, send_flags)?;
    }
    poll_cq_and_check(send_cq, &mut wcs)?;

    Ok(())
}
