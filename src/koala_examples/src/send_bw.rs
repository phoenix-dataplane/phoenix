use std::time;

use structopt::StructOpt;

use libkoala::verbs::{QpCapability, QpInitAttr, QpType, SendFlags, WcStatus};
use libkoala::cm;

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
            max_send_wr: 1,
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

    let mut recv_msg = vec![0u8; opts.msg_size];
    let recv_mr = id.reg_msgs(&recv_msg)?;

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
    let mut cqe = 0;
    while cqe < opts.warm_iters + opts.total_iters {
        cq.poll_cq(&mut wc)?;
        cqe += wc.len();
        for c in &wc {
            assert_eq!(c.status, WcStatus::Success);
        }
    }

    println!("{:?}", &recv_msg[..100.min(opts.msg_size)]);

    assert_eq!(&recv_msg, &vec![42u8; opts.msg_size],);
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
            max_recv_wr: 1,
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

    let send_flags = SendFlags::SIGNALED;
    let send_msg = vec![42u8; opts.msg_size];
    let send_mr = id.reg_msgs(&send_msg)?;
    for _ in 0..opts.warm_iters {
        id.post_send(0, &send_msg, &send_mr, send_flags)?;
    }

    let mut wc = Vec::with_capacity(32);
    let cq = &id.qp.as_ref().unwrap().send_cq;
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
        id.post_send(0, &send_msg, &send_mr, send_flags)?;
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
