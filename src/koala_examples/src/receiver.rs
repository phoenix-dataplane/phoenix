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
            max_recv_wr: 16,
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
    let id = listen_id.get_request().expect("Get request failed!");

    let mut recv_msg = vec![0u8; 128];
    let recv_mr = id.reg_msgs(&recv_msg).expect("Memory registration failed!");

    unsafe {
        id.post_recv(0, &mut recv_msg, &recv_mr)
            .expect("Post recv failed!");
    }

    id.accept(None).expect("Accept failed!");

    let wc_recv = id.get_recv_comp().expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    let send_flags = SendFlags::SIGNALED;
    let send_msg = "Hello koala client!";
    let send_mr = id
        .reg_msgs(send_msg.as_bytes())
        .expect("Memory registration failed!");
    id.post_send(0, send_msg.as_bytes(), &send_mr, send_flags)
        .expect("Post send failed!");

    let wc_send = id.get_send_comp().expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);

    println!("{:?}", recv_msg);

    assert_eq!(
        &recv_msg[..send_msg.len()],
        "Hello koala server!".as_bytes()
    );
    Ok(())
}
