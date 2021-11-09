use getopt;
use interface::{
    addrinfo::{AddrFamily, AddrInfo, AddrInfoFlags, AddrInfoHints, PortSpace},
    QpCapability, QpInitAttr, QpType, WcStatus,
};
use libkoala::{cm, koala_register, verbs, Context};
use rand;
use std::time::SystemTime;

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn init() -> (Context, AddrInfo) {
    let ctx = koala_register().expect("register failed!");

    let hints = AddrInfoHints::new(
        AddrInfoFlags::default(),
        Some(AddrFamily::Inet),
        QpType::RC,
        PortSpace::TCP,
    );

    let ai = cm::getaddrinfo(
        &ctx,
        Some(&SERVER_ADDR),
        Some(&SERVER_PORT.to_string()),
        Some(&hints),
    )
    .expect("getaddrinfo");

    eprintln!("ai: {:?}", ai);

    (ctx, ai)
}

fn run_delay_client(size: usize, num: usize) -> Result<(), Box<dyn std::error::Error>> {
    let (ctx, ai) = init();

    let qp_init_attr = QpInitAttr {
        qp_context: None,
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: QpCapability {
            max_send_wr: 1024,
            max_recv_wr: 128,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let id = cm::create_ep(&ctx, &ai, None, Some(&qp_init_attr))?;

    let mut recv_msg: Vec<u8> = Vec::with_capacity(num);
    recv_msg.resize(recv_msg.capacity(), 1);
    let recv_mr = cm::reg_msgs(&ctx, &id, &recv_msg).expect("Memory registration failed!");

    unsafe {
        verbs::post_recv(&ctx, &id, 0, &mut recv_msg, &recv_mr).expect("Post recv failed!");
    }

    cm::connect(&ctx, &id, None).expect("Connect failed!");

    let send_flags = Default::default();
    let send_msg: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();
    let send_mr = cm::reg_msgs(&ctx, &id, &send_msg).expect("Memory registration failed!");

    let start_time = SystemTime::now();
    for i in 0..num {
        verbs::post_send(&ctx, &id, 0, &send_msg, &send_mr, send_flags).expect("Post send failed!");
    }

    let mut cnt = 0;
    while cnt < num {
        let recv_wcs = verbs::poll_cq(&ctx, num as i32).expect("poll cq failed");
        for wc in &recv_wcs {
            assert_eq!(wc.status, WcStatus::Success);
        }
        cnt += recv_wcs.len();
    }
    let duration = start_time.elapsed().expect("Timestamp error");
    let delay = duration.as_nanos() / num as u128;

    println!("{}ns", delay);

    Ok(())
}

fn run_delay_server(size: usize, num: usize) {}

fn run_bandwidth(size: usize) {}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().collect();
    let mut opts = getopt::Parser::new(&args, "ct:l:n:");

    let mut client = false;
    let mut test = String::from("delay");
    let mut size = None;
    let mut num = None;
    loop {
        match opts.next().transpose()? {
            None => break,
            Some(opt) => match opt {
                getopt::Opt('c', None) => client = true,
                getopt::Opt('t', Some(string)) => test = string,
                getopt::Opt('l', Some(string)) => size = string.parse::<usize>().ok(),
                getopt::Opt('n', Some(string)) => num = string.parse::<usize>().ok(),
                _ => unreachable!(),
            },
        }
    }

    let args = args.split_off(opts.index());

    if test == "delay" || test == "d" {
        let size = size.unwrap_or(1024); //KB
        let num = num.unwrap_or(1000);
        if client {
            run_delay_client(size, num);
        } else {
            run_delay_server(size, num);
        }
    } else if test == "bandwidth" {
        let size = size.unwrap_or(10 * 1024); //MB
        let num = num.unwrap_or(1);
        if client {
            run_delay_client(size, num);
        } else {
            run_delay_server(size, num);
        }
    }

    Ok(())
}
