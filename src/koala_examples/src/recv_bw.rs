use libkoala::cm::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace};
use libkoala::verbs::{QpType, SendFlags, WcStatus};
use libkoala::{cm, verbs};

const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hints = AddrInfoHints::new(
        AddrInfoFlags::PASSIVE,
        Some(AddrFamily::Inet),
        QpType::RC,
        PortSpace::TCP,
    );

    let ai =
        cm::getaddrinfo(None, Some(&SERVER_PORT.to_string()), Some(&hints)).expect("getaddrinfo");

    eprintln!("ai: {:?}", ai);

    let qp_init_attr = verbs::QpInitAttr {
        qp_context: Some(&3),
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: verbs::QpCapability {
            max_send_wr: 16,
            max_recv_wr: 1024,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let listen_id = cm::CmId::create_ep(&ai, None, Some(&qp_init_attr))?;

    eprintln!("listen_id created");

    listen_id.listen(16).expect("Listen failed!");
    let id = listen_id.get_requst().expect("Get request failed!");

    let mut recv_msg = vec![0u8; 65536];
    let recv_mr = id.reg_msgs(&recv_msg).expect("Memory registration failed!");

    for i in 0..1000 {
        unsafe {
            id.post_recv(0, &mut recv_msg, &recv_mr)
                .expect("Post recv failed!");
        }
    }

    id.accept(None).expect("Accept failed!");

    // busy poll
    let mut wc = Vec::with_capacity(32);
    let cq = id.qp.unwrap().recv_cq;
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

    println!("{:?}", &recv_msg[..100]);

    assert_eq!(&recv_msg, &vec![42u8; 65536],);
    Ok(())
}
