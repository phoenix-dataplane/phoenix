use libkoala::verbs::{SendFlags, WcStatus};
use libkoala::{cm, verbs};
use std::time;

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ai = cm::getaddrinfo(Some(&SERVER_ADDR), Some(&SERVER_PORT.to_string()), None)
        .expect("getaddrinfo");

    eprintln!("ai: {:?}", ai);

    let qp_init_attr = verbs::QpInitAttr {
        qp_context: Some(&3),
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: verbs::QpCapability {
            max_send_wr: 1024,
            max_recv_wr: 128,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: verbs::QpType::RC,
        sq_sig_all: false,
    };

    let id = cm::CmId::create_ep(&ai, None, Some(&qp_init_attr))?;

    eprintln!("cmid created");

    id.connect(None).expect("Connect failed!");

    let ts = time::Instant::now();

    let send_flags = SendFlags::SIGNALED;
    let send_msg = vec![42u8; 65536];
    let send_mr = id.reg_msgs(&send_msg).expect("Memory registration failed!");
    for _ in 0..1000 {
        id.post_send(0, &send_msg, &send_mr, send_flags)
            .expect("Post send failed!");
    }

    let dura = ts.elapsed();
    eprintln!("post_send finished, duration: {:?}", dura,);

    // busy poll
    let mut wc = Vec::with_capacity(32);
    let cq = id.qp.unwrap().send_cq;
    let mut cqe = 0;
    while cqe < 1000 {
        loop {
            cq.poll_cq(&mut wc).expect("poll_cq failed");
            if !wc.is_empty() {
                break;
            }
        }
        cqe += wc.len();
        for c in &wc {
            assert_eq!(c.status, WcStatus::Success);
        }
    }

    let dura = ts.elapsed();
    eprintln!(
        "duration: {:?}, bandwidth: {} Gb/s",
        dura,
        8e-9 * 1000. * 65536. / dura.as_secs_f64()
    );

    Ok(())
}
