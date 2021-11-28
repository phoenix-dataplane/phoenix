use libkoala::cm::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace};
use libkoala::verbs::{MemoryRegion, QpType, SendFlags, WcStatus};
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
            max_recv_wr: 16,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let listener = cm::CmIdListener::bind(("0.0.0.0", SERVER_PORT)).expect("Listener bind failed");
    eprintln!("listen_id created");

    let mut builder = listener.get_request().expect("Get request failed!");
    eprintln!("Get a connect request");
    let pre_id = builder
        .set_qp_init_attr(&qp_init_attr)
        .build()
        .expect("Create QP failed!");
    eprintln!("QP created");

    let mut recv_mr: MemoryRegion<u8> =
        pre_id.alloc_msgs(128).expect("Memory registration failed!");

    unsafe {
        pre_id
            .post_recv(&mut recv_mr, .., 0)
            .expect("Post recv failed!");
    }

    let id = pre_id.accept(None).expect("Accept failed!");
    eprintln!("Connection established");

    let wc_recv = id.get_recv_comp().expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    let send_msg = "Hello koala client!";
    let send_mr = {
        let mut mr = id
            .alloc_msgs(send_msg.len())
            .expect("Memory registration failed!");
        mr.copy_from_slice(send_msg.as_bytes());
        mr
    };
    id.post_send(&send_mr, .., 0, SendFlags::SIGNALED)
        .expect("Post send failed!");

    let wc_send = id.get_send_comp().expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);

    println!("{:?}", recv_mr.as_slice());

    assert_eq!(&recv_mr[..send_mr.len()], "Hello koala server!".as_bytes());
    Ok(())
}
