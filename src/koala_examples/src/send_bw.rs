use std::time;

use structopt::StructOpt;

use libkoala::cm;
use libkoala::verbs::{MemoryRegion, SendFlags, WcStatus};

const SERVER_PORT: &str = "5000";

#[derive(StructOpt, Debug, Clone)]
#[structopt(about = "Koala send/recv bandwidth")]
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
    #[structopt(short, long, default_value = "65536")]
    pub msg_size: usize,
}

fn run_server(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    let listener = cm::CmIdBuilder::new()
        .set_max_recv_wr((opts.warm_iters + opts.total_iters) as _)
        .bind(("0.0.0.0", opts.port))
        .expect("Listener bind failed");
    eprintln!("listen_id created");

    let builder = listener.get_request().expect("Get request failed!");
    eprintln!("Get a connect request");
    let pre_id = builder.build().expect("Create QP failed!");
    eprintln!("QP created");

    let mut recv_mr: MemoryRegion<u8> = pre_id
        .alloc_msgs(opts.msg_size)
        .expect("Memory registration failed!");

    for _ in 0..opts.warm_iters + opts.total_iters {
        unsafe {
            pre_id.post_recv(&mut recv_mr, .., 0)?;
        }
    }

    let id = pre_id.accept(None).expect("Accept failed!");
    eprintln!("Accepted a connection");

    // busy poll
    let mut wc = Vec::with_capacity(32);
    let cq = &id.qp().recv_cq;
    let mut cqe = 0;
    while cqe < opts.warm_iters + opts.total_iters {
        cq.poll_cq(&mut wc)?;
        cqe += wc.len();
        for c in &wc {
            assert_eq!(c.status, WcStatus::Success);
        }
    }

    println!("{:?}", &recv_mr[..100.min(opts.msg_size)]);

    assert_eq!(recv_mr.as_slice(), &vec![42u8; opts.msg_size],);
    Ok(())
}

fn run_client(opts: &Opts) -> Result<(), Box<dyn std::error::Error>> {
    let builder = cm::CmIdBuilder::new()
        .set_max_send_wr((opts.warm_iters + opts.total_iters) as _)
        .resolve_route((opts.connect.as_deref().unwrap(), opts.port))
        .expect("Route resolve failed!");
    eprintln!("Route resolved");

    let pre_id = builder.build().expect("Create QP failed!");
    eprintln!("QP created");

    let id = pre_id.connect(None).expect("Connect failed!");
    eprintln!("Connected to remote side");

    let send_flags = SendFlags::SIGNALED;
    let send_mr = {
        let mut mr = id
            .alloc_msgs(opts.msg_size)
            .expect("Memory registration failed!");
        mr.fill(42u8);
        mr
    };

    for _ in 0..opts.warm_iters {
        id.post_send(&send_mr, .., 0, send_flags)?;
    }

    let mut wc = Vec::with_capacity(32);
    let cq = &id.qp().send_cq;
    let mut cqe = 0;
    while cqe < opts.warm_iters {
        cq.poll_cq(&mut wc)?;
        cqe += wc.len();
        for c in &wc {
            assert_eq!(c.status, WcStatus::Success, "{:?}", c);
        }
    }

    let ts = time::Instant::now();

    for _ in 0..opts.total_iters {
        id.post_send(&send_mr, .., 0, send_flags)?;
    }

    let dura = ts.elapsed();
    eprintln!("post_send finished, duration: {:?}", dura,);

    // busy poll
    let mut cqe = 0;
    while cqe < opts.total_iters {
        cq.poll_cq(&mut wc)?;
        cqe += wc.len();
        for c in &wc {
            assert_eq!(c.status, WcStatus::Success, "{:?}", c);
        }
    }

    let dura = ts.elapsed();
    eprintln!(
        "duration: {:?}, bandwidth: {} Gb/s",
        dura,
        8e-9 * opts.total_iters as f64 * opts.msg_size as f64 / dura.as_secs_f64()
    );

    Ok(())
}

fn main() {
    let opts = Opts::from_args();
    if opts.connect.is_some() {
        run_client(&opts).unwrap();
    } else {
        run_server(&opts).unwrap();
    }
}
