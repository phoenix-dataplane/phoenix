#![feature(nonnull_slice_from_raw_parts)]
#![feature(allocator_api)]
use std::time;

use std::alloc::{Allocator, Layout, AllocError};
use std::ptr::NonNull;
use std::ptr;

use structopt::StructOpt;

use libkoala::verbs::{QpCapability, QpInitAttr, QpType, SendFlags, WcStatus};
use libkoala::{cm, verbs};

const SERVER_PORT: &str = "5000";

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Koala send/recv latency.")]
pub struct Opts {
    /// The address to connect, can be an IP address or domain name.
    #[structopt(short, long)]
    pub connect: Option<String>,

    /// The port number to use.
    #[structopt(short, long, default_value = SERVER_PORT)]
    pub port: u16,

    /// Total number of iterations.
    #[structopt(short, long, default_value = "1000")]
    pub total_iters: usize,

    /// Number of warmup iterations.
    #[structopt(short, long, default_value = "100")]
    pub warm_iters: usize,

    /// Message size.
    #[structopt(short, long, default_value = "8")]
    pub msg_size: usize,
}

struct AlignedAllocator;

unsafe impl Allocator for AlignedAllocator {
    fn allocate(
        &self,
        layout: Layout,
    ) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        let mut addr = ptr::null_mut();
        let err = unsafe { libc::posix_memalign(&mut addr as *mut _ as _, 4096, layout.size()) };
        if err != 0 {
            return Err(AllocError);
        }

        let ptr = NonNull::new(addr).unwrap();
        Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _layout: Layout) {
        libc::free(ptr.as_ptr() as _);
    }
}

fn run_server(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    use libkoala::cm::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace};

    let hints = AddrInfoHints::new(
        AddrInfoFlags::PASSIVE,
        Some(AddrFamily::Inet),
        QpType::RC,
        PortSpace::TCP,
    );

    let ai =
        cm::getaddrinfo(None, Some(&opts.port.to_string()), Some(&hints)).expect("getaddrinfo");

    eprintln!("ai: {:?}", ai);

    let qp_init_attr = QpInitAttr {
        qp_context: Some(&3),
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: QpCapability {
            max_send_wr: (opts.warm_iters + opts.total_iters) as _,
            max_recv_wr: (opts.warm_iters + opts.total_iters) as _,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let listen_id = cm::CmId::create_ep(&ai, None, Some(&qp_init_attr))?;

    eprintln!("listen_id created");

    listen_id.listen(16)?;
    let id = listen_id.get_request()?;

    let mut recv_msg = Vec::with_capacity_in(opts.msg_size, AlignedAllocator);
    recv_msg.resize(opts.msg_size, 0u8);
    let recv_mr = id.reg_msgs(&recv_msg)?;

    let mut send_msg = Vec::with_capacity_in(opts.msg_size, AlignedAllocator);
    send_msg.resize(opts.msg_size, 42u8);
    let send_mr = id.reg_msgs(&send_msg)?;

    for _ in 0..opts.warm_iters + opts.total_iters {
        unsafe {
            id.post_recv(0, &mut recv_msg, &recv_mr)?;
        }
    }

    id.accept(None)?;
    eprintln!("accepted a connection");

    // busy poll
    let mut wc = Vec::with_capacity(32);
    let cq = &id.qp.as_ref().unwrap().recv_cq;
    let mut rcnt = 0;
    while rcnt < opts.warm_iters + opts.total_iters {
        cq.poll_cq(&mut wc)?;
        for c in &wc {
            // eprintln!("rcnt: {}, wc: {:?}", rcnt, c);
            assert_eq!(c.status, WcStatus::Success);
            rcnt += 1;
            let send_flags = if rcnt == opts.warm_iters + opts.total_iters {
                SendFlags::SIGNALED
            } else {
                Default::default()
            };
            id.post_send(0, &send_msg, &send_mr, send_flags)?;
        }
    }

    let send_wc = id.get_send_comp()?;
    eprintln!("get_send_comp, wc: {:?}", send_wc);
    assert_eq!(send_wc.status, WcStatus::Success);

    println!("{:?}", &recv_msg[..100.min(opts.msg_size)]);

    assert_eq!(recv_msg.as_slice(), vec![42u8; opts.msg_size].as_slice());
    Ok(())
}

fn run_client(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    let ai = cm::getaddrinfo(
        Some(&opts.connect.as_deref().unwrap()),
        Some(&opts.port.to_string()),
        None,
    )?;

    eprintln!("ai: {:?}", ai);

    let qp_init_attr = QpInitAttr {
        qp_context: Some(&3),
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: QpCapability {
            max_send_wr: (opts.warm_iters + opts.total_iters) as _,
            max_recv_wr: (opts.warm_iters + opts.total_iters) as _,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let id = cm::CmId::create_ep(&ai, None, Some(&qp_init_attr))?;

    eprintln!("cmid created");

    id.connect(None)?;
    eprintln!("connected to remote side");

    // post receives in advance
    let mut recv_msg = Vec::with_capacity_in(opts.msg_size, AlignedAllocator);
    recv_msg.resize(opts.msg_size, 0u8);
    let recv_mr = id.reg_msgs(&recv_msg)?;

    for _ in 0..opts.warm_iters + opts.total_iters {
        unsafe {
            id.post_recv(0, &mut recv_msg, &recv_mr)?;
        }
    }

    let no_signal = Default::default();
    let mut send_msg = Vec::with_capacity_in(opts.msg_size, AlignedAllocator);
    send_msg.resize(opts.msg_size, 42u8);
    let send_mr = id.reg_msgs(&send_msg)?;

    // start sending
    id.post_send(0, &send_msg, &send_mr, no_signal)?;

    let start_ts = time::Instant::now();
    let mut ts = time::Instant::now();
    let mut stats = Vec::with_capacity_in(opts.total_iters, AlignedAllocator);

    let mut wc = Vec::with_capacity(1);
    let cq = &id.qp.as_ref().unwrap().recv_cq;
    let mut scnt = 1;
    let mut rcnt = 0;
    while rcnt < opts.warm_iters + opts.total_iters {
        cq.poll_cq(&mut wc)?;
        for c in &wc {
            // eprintln!("rcnt: {}, wc: {:?}", rcnt, c);
            assert_eq!(c.status, WcStatus::Success, "{:?}", c);
            rcnt += 1;

            // record the latency from post_send to receive completion is polled
            if rcnt > opts.warm_iters {
                stats.push(ts.elapsed());
            }

            if rcnt >= opts.warm_iters {
                // start timing
                ts = time::Instant::now();
            }

            if scnt >= opts.warm_iters + opts.total_iters {
                continue;
            }
            let send_flags = if scnt + 1 == opts.warm_iters + opts.total_iters {
                SendFlags::SIGNALED
            } else {
                no_signal
            };
            id.post_send(0, &send_msg, &send_mr, send_flags)?;
            scnt += 1;
        }
    }

    let send_wc = id.get_send_comp()?;
    eprintln!("get_send_comp, wc: {:?}", send_wc);
    assert_eq!(send_wc.status, WcStatus::Success);
    stats.push(ts.elapsed());

    let dura = start_ts.elapsed();
    stats.sort();
    eprintln!(
        "duration: {:?}, avg: {:?}, median: {:?}, min: {:?}, max: {:?}",
        dura,
        get_avg(&stats),
        get_median(&stats),
        get_min(&stats),
        get_max(&stats),
    );

    Ok(())
}

fn get_avg(stats: &[time::Duration]) -> time::Duration {
    let s: time::Duration = stats.iter().sum();
    s / stats.len() as u32
}

fn get_median(stats: &[time::Duration]) -> time::Duration {
    assert!(!stats.is_empty());
    stats[stats.len() / 2]
}

fn get_min(stats: &[time::Duration]) -> time::Duration {
    assert!(!stats.is_empty());
    stats[0]
}

fn get_max(stats: &[time::Duration]) -> time::Duration {
    assert!(!stats.is_empty());
    *stats.last().unwrap()
}

fn main() {
    let opts = Opts::from_args();

    if opts.connect.is_some() {
        run_client(&opts).unwrap();
    } else {
        run_server(&opts).unwrap();
    }
}
