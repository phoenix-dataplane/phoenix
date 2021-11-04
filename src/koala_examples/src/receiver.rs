use interface::{
    addrinfo::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace},
    QpCapability, QpInitAttr, QpType, WcStatus, SendFlags,
};
use libkoala::{cm, koala_register, verbs};

const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = koala_register().expect("register failed!");

    let hints = AddrInfoHints::new(
        AddrInfoFlags::PASSIVE,
        Some(AddrFamily::Inet),
        QpType::RC,
        PortSpace::TCP,
    );

    let ai = cm::getaddrinfo(
        &ctx,
        None,
        Some(&SERVER_PORT.to_string()),
        Some(&hints),
    )
    .expect("getaddrinfo");

    eprintln!("ai: {:?}", ai);

    let qp_init_attr = QpInitAttr {
        qp_context: Some(&3),
        send_cq: None,
        recv_cq: None,
        srq: None,
        cap: QpCapability {
            max_send_wr: 16,
            max_recv_wr: 16,
            max_send_sge: 1,
            max_recv_sge: 1,
            max_inline_data: 128,
        },
        qp_type: QpType::RC,
        sq_sig_all: false,
    };

    let listen_id = cm::create_ep(&ctx, &ai, None, Some(&qp_init_attr))?;

    eprintln!("listen_id: {:?}", listen_id);

    cm::listen(&ctx, &listen_id, 16).expect("Listen failed!");
    let id = cm::get_requst(&ctx, &listen_id).expect("Get request failed!");

    let mut recv_msg: Vec<u8> = Vec::with_capacity(128);
    recv_msg.resize(recv_msg.capacity(), 0);
    let recv_mr = cm::reg_msgs(&ctx, &id, &recv_msg).expect("Memory registration failed!");

    unsafe {
        verbs::post_recv(&ctx, &id, 0, &mut recv_msg, &recv_mr).expect("Post recv failed!");
    }

    cm::accept(&ctx, &id, None).expect("Accept failed!");

    let wc_recv = verbs::get_recv_comp(&ctx, &id).expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    let send_flags = SendFlags::SIGNALED;
    let send_msg = "Hello koala client!";
    let send_mr =
        cm::reg_msgs(&ctx, &id, send_msg.as_bytes()).expect("Memory registration failed!");
    verbs::post_send(&ctx, &id, 0, send_msg.as_bytes(), &send_mr, send_flags)
        .expect("Connect failed!");

    let wc_send = verbs::get_send_comp(&ctx, &id).expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);

    println!("{:?}", recv_msg);

    assert_eq!(&recv_msg[..send_msg.len()], "Hello koala server!".as_bytes());
    Ok(())
}
