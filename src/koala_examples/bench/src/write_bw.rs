use std::net::ToSocketAddrs;
use std::thread;
use std::time::Instant;

use crate::util::{print_bw, Context, CTX_POLL_BATCH};
use libkoala::verbs::{CompletionQueue, MemoryRegion, RemoteKey, SendFlags, VerbsContext};
use libkoala::{cm, verbs, Error};

#[derive(Debug)]
struct Channel {
    id: cm::CmId,
    local_mr: MemoryRegion<u8>,
    rkey: RemoteKey,
    send_flags: SendFlags,
    scnt: usize,
    ccnt: usize,
    max_send_wr: usize,
    rank: usize,
}

impl Channel {
    #[inline]
    fn post_write(&mut self) -> Result<(), Error> {
        let send_flags = self.send_flags | SendFlags::SIGNALED;

        assert!(self.can_send());

        // It should be fine because local_mr and id remain valid (cannot be dropped)
        // within this object.
        self.scnt += 1;
        unsafe {
            self.id.post_write(
                &self.local_mr,
                ..,
                self.rank as u64,
                send_flags,
                self.rkey,
                0,
            )
        }
    }

    #[inline]
    fn can_send(&self) -> bool {
        self.scnt < self.ccnt + self.max_send_wr
    }

    fn close_remote(&self) -> Result<(), Error> {
        let send_flags = self.send_flags | SendFlags::SIGNALED;
        assert!(self.can_send());
        assert!(self.local_mr.len() >= 1);
        unsafe {
            self.id.post_send(&self.local_mr, ..1, 0, send_flags)?;
        }
        assert!(self.id.get_send_comp()?.success());
        Ok(())
    }
}

fn client_connect<A: ToSocketAddrs>(
    qp_id: usize,
    server_addr: A,
    ctx: &Context,
    cq: &CompletionQueue,
) -> Result<Channel, Error> {
    // Resolve route
    let mut builder = cm::CmId::builder()
        .set_send_cq(cq)
        .set_recv_cq(cq)
        .resolve_route(server_addr)
        .expect("Route resolve failed");
    eprintln!("Route resolved");

    // Create QP
    let pre_id = builder.set_cap(ctx.cap).build().expect("Create QP failed!");

    // Allocate memory regions
    let local_mr: MemoryRegion<u8> = pre_id.alloc_msgs(ctx.opt.size)?;

    // Acquire remote mr
    let mut remote_rkey_mr: MemoryRegion<RemoteKey> = pre_id.alloc_msgs(1)?;
    unsafe {
        pre_id.post_recv(&mut remote_rkey_mr, .., 0)?;
    }

    // Connect
    let id = pre_id.connect(None)?;

    assert!(id.get_recv_comp()?.success());

    // Whether to send with inline data
    let send_flags = if ctx.cap.max_inline_data as usize >= ctx.opt.size {
        SendFlags::INLINE
    } else {
        SendFlags::empty()
    };

    Ok(Channel {
        id,
        local_mr,
        rkey: remote_rkey_mr[0],
        send_flags,
        scnt: 0,
        ccnt: 0,
        max_send_wr: ctx.cap.max_send_wr as usize,
        rank: qp_id,
    })
}

fn run_client_thread(tid: usize, server_tid: usize, ctx: Context) -> Result<(), Error> {
    let mut channels = Vec::with_capacity(ctx.opt.num_qp);

    // Create a global CQ
    let cq = VerbsContext::default_verbs_contexts()[0].create_cq(65536, 0)?;

    // Establish connections
    let server_addr = (ctx.opt.ip.as_str(), ctx.opt.port + server_tid as u16);
    for qp_id in 0..ctx.opt.num_qp {
        eprintln!(
            "tid: {}, qp_id: {} is connecting to {:?}",
            tid, qp_id, server_addr
        );

        channels.push(client_connect(qp_id, server_addr, &ctx, &cq)?);
    }

    // Start the mainloop
    let mut tposted = Vec::with_capacity(ctx.opt.num);
    let mut tcompleted = Vec::with_capacity(ctx.opt.num);
    let mut wcs = Vec::with_capacity(CTX_POLL_BATCH);
    let mut scnt = 0;
    let mut ccnt = 0;
    while scnt < ctx.opt.num || ccnt < ctx.opt.num {
        if scnt < ctx.opt.num && channels[scnt % ctx.opt.num_qp].can_send() {
            tposted.push(Instant::now());
            channels[scnt % ctx.opt.num_qp].post_write()?;
            scnt += 1;
        }
        if ccnt < ctx.opt.num {
            cq.poll_cq(&mut wcs).expect("Poll cq failed!");
            ccnt += wcs.len();
            for wc in &wcs {
                assert!(wc.success(), "wc: {:?}", wc);
                tcompleted.push(Instant::now());
                channels[wc.wr_id as usize].ccnt += 1;
            }
        }
    }

    eprintln!("mainloop finished");

    // Tell the server to close the connection
    for c in &channels {
        c.close_remote().unwrap();
    }

    print_bw(&ctx, &tposted, &tcompleted);
    Ok(())
}

pub fn run_client(ctx: &Context) -> Result<(), Error> {
    let mut handles = Vec::new();

    for tid in 0..ctx.opt.num_client_threads {
        let server_tid = tid % ctx.opt.num_server_threads;
        let ctx = ctx.clone();

        handles.push(thread::spawn(move || {
            run_client_thread(tid, server_tid, ctx)
        }));
    }

    for h in handles {
        let _ = h.join().unwrap();
    }

    Ok(())
}

fn server_accept(
    listener: &cm::CmIdListener,
    ctx: &Context,
    cq: &CompletionQueue,
) -> Result<(cm::CmId, MemoryRegion<u8>), Error> {
    let mut builder = listener.get_request().expect("Get request failed");
    let pre_id = builder
        .set_cap(ctx.cap)
        .set_send_cq(cq)
        .set_recv_cq(cq)
        .build()
        .expect("Create QP failed");

    let mut local_mr: MemoryRegion<u8> = pre_id.alloc_write(ctx.opt.size)?;

    // Post recv for the close signal
    unsafe {
        pre_id.post_recv(&mut local_mr, .., 0)?;
    }

    let id = pre_id.accept(None)?;

    // Push rkey to remote
    let mut rkey_mr: MemoryRegion<RemoteKey> = id.alloc_msgs(1)?;
    rkey_mr[0] = local_mr.rkey();
    unsafe {
        id.post_send(&mut rkey_mr, .., 0, SendFlags::SIGNALED)?;
    }

    assert!(id.get_send_comp()?.success());

    Ok((id, local_mr))
}

fn run_server_thread(tid: usize, ctx: Context) -> Result<(), Error> {
    // Create a global CQ
    let cq = VerbsContext::default_verbs_contexts()[0].create_cq(65536, 0)?;

    // Each thread has one listener
    let listen_addr = (ctx.opt.ip.as_str(), ctx.opt.port + tid as u16);
    let listener = cm::CmIdListener::bind(listen_addr).expect("Listener bind failed");

    let server_qp_num = ctx.opt.num_client_threads * ctx.opt.num_qp / ctx.opt.num_server_threads;
    let mut channels = Vec::new();

    for _ in 0..server_qp_num {
        channels.push(server_accept(&listener, &ctx, &cq)?);
    }

    eprintln!("server waiting for end signal...");
    // Wait for the recv
    for i in 0..server_qp_num {
        let wc = channels[i].0.get_recv_comp()?;
        assert!(wc.success(), "wc: {:?}", wc);
        assert_eq!(wc.opcode, verbs::WcOpcode::Recv);
    }

    eprintln!("server end");

    Ok(())
}

pub fn run_server(ctx: &Context) -> Result<(), Error> {
    let mut handles = Vec::new();

    for tid in 0..ctx.opt.num_server_threads {
        let ctx = ctx.clone();

        handles.push(thread::spawn(move || run_server_thread(tid, ctx)));
    }

    for h in handles {
        let _ = h.join().unwrap();
    }

    Ok(())
}
