use std::time;

use structopt::StructOpt;

use libkoala::cm;
use libkoala::verbs::{MemoryRegion, QpCapability, QpInitAttr, QpType, SendFlags, WcStatus};

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

fn run_server(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    let listener = cm::CmIdListener::bind(("0.0.0.0", opts.port)).expect("Listener bind failed");
    eprintln!("listen_id created");

    let qp_init_attr = QpInitAttr {
        cap: QpCapability {
            max_send_wr: (opts.total_iters + opts.warm_iters) as u32,
            max_recv_wr: (opts.total_iters + opts.warm_iters) as u32,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
        ..Default::default()
    };

    let mut builder = listener.get_request().expect("Get request failed!");
    eprintln!("Get a connect request");
    let pre_id = builder
        .set_qp_init_attr(&qp_init_attr)
        .build()
        .expect("Create QP failed!");
    eprintln!("QP created");

    let mut recv_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(opts.msg_size)
        .expect("Memory registration failed!");

    let send_mr = {
        let mut mr = pre_id
            .alloc_msgs(opts.msg_size)
            .expect("Memory registration failed!");
        mr.fill(42u8);
        mr
    };

    for _ in 0..opts.warm_iters + opts.total_iters {
        unsafe {
            pre_id.post_recv(&mut recv_mr, .., 0)?;
        }
    }

    let id = pre_id.accept(None).expect("Accept failed!");
    eprintln!("Accepted a connection");

    // busy poll
    let mut wc = Vec::with_capacity(32);
    let cq = id.qp().recv_cq();
    let mut rcnt = 0;
    while rcnt < opts.warm_iters + opts.total_iters {
        cq.poll_cq(&mut wc)?;
        for c in &wc {
            // eprintln!("rcnt: {}, wc: {:?}", rcnt, c);
            assert_eq!(c.status, WcStatus::Success);
            rcnt += 1;
            let send_flags = if rcnt == opts.warm_iters + opts.total_iters {
                SendFlags::SIGNALED | SendFlags::INLINE
            } else {
                SendFlags::INLINE
            };
            unsafe {
                id.post_send(&send_mr, .., 0, send_flags)?;
            }
        }
    }

    let send_wc = id.get_send_comp()?;
    eprintln!("get_send_comp, wc: {:?}", send_wc);
    assert_eq!(send_wc.status, WcStatus::Success);

    println!("{:?}", &recv_mr[..100.min(opts.msg_size)]);

    assert_eq!(recv_mr.as_slice(), &vec![42u8; opts.msg_size]);
    Ok(())
}

fn run_client(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    let builder = cm::CmIdBuilder::new()
        .set_max_send_wr((opts.total_iters + opts.warm_iters) as _)
        .set_max_recv_wr((opts.total_iters + opts.warm_iters) as _)
        .resolve_route((opts.connect.as_deref().unwrap(), opts.port))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");

    let pre_id = builder.build().expect("Create QP failed!");
    eprintln!("QP created");

    let id = pre_id.connect(None).expect("Connect failed!");
    eprintln!("Connected to remote side");

    // post receives in advance
    let mut recv_mr: MemoryRegion<u8> = id
        .alloc_msgs(opts.msg_size)
        .expect("Memory registration failed!");

    for _ in 0..opts.warm_iters + opts.total_iters {
        unsafe {
            id.post_recv(&mut recv_mr, .., 0)?;
        }
    }

    let no_signal = SendFlags::INLINE;
    let send_mr = {
        let mut mr = id
            .alloc_msgs(opts.msg_size)
            .expect("Memory registration failed!");
        mr.fill(42u8);
        mr
    };

    // start sending
    unsafe {
        id.post_send(&send_mr, .., 0, no_signal)?;
    }

    let start_ts = time::Instant::now();
    let mut ts = time::Instant::now();
    let mut stats = Vec::with_capacity(opts.total_iters);

    let mut wc = Vec::with_capacity(1);
    let cq = id.qp().recv_cq();
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
                SendFlags::SIGNALED | SendFlags::INLINE
            } else {
                no_signal
            };
            unsafe {
                id.post_send(&send_mr, .., 0, send_flags)?;
            }
            scnt += 1;
        }
    }

    let send_wc = id.get_send_comp()?;
    eprintln!("get_send_comp, wc: {:?}", send_wc);
    assert_eq!(send_wc.status, WcStatus::Success);
    stats.push(ts.elapsed());

    let dura = start_ts.elapsed();
    stats.sort();
    assert!(!stats.is_empty());

    // One-way latency
    let stats: Vec<_> = stats.into_iter().map(|x| x / 2).collect();

    println!(
        "duration: {:?}, #iters: {}, avg: {:?}, min: {:?}, median: {:?}, P95: {:?}, P99: {:?}, max: {:?}",
        dura,
        opts.total_iters,
        stats.iter().sum::<time::Duration>() / stats.len() as u32,
        stats[0],
        stats[(0.5 * stats.len() as f64) as usize],
        stats[(0.95 * stats.len() as f64) as usize],
        stats[(0.99 * stats.len() as f64) as usize],
        stats[stats.len() - 1],
    );

    Ok(())
}

fn main() {
    let opts = Opts::from_args();

    // scheduler::set_self_affinity(scheduler::CpuSet::single(20)).unwrap();
    if opts.connect.is_some() {
        run_client(&opts).unwrap();
    } else {
        run_server(&opts).unwrap();
    }
}
