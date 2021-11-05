use interface::{
    addrinfo::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace},
    QpCapability, QpInitAttr, QpType, SendFlags, WcStatus,
};
use libkoala::{cm, koala_register, verbs};

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let qp_init_attr = QpInitAttr {
        qp_context: Some(&3),
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

    // let id = cm::create_ep(&ctx, ai, None, None)?;
    let id = cm::create_ep(&ctx, &ai, None, Some(&qp_init_attr))?;

    eprintln!("id: {:?}", id);

    let mut recv_msg: Vec<u8> = Vec::with_capacity(128);
    recv_msg.resize(recv_msg.capacity(), 1);
    let recv_mr = cm::reg_msgs(&ctx, &id, &recv_msg).expect("Memory registration failed!");

    unsafe {
        verbs::post_recv(&ctx, &id, 0, &mut recv_msg, &recv_mr).expect("Post recv failed!");
    }

    cm::connect(&ctx, &id, None).expect("Connect failed!");

    let send_flags = SendFlags::SIGNALED;
    let send_msg = "Hello koala server!";
    let send_mr =
        cm::reg_msgs(&ctx, &id, send_msg.as_bytes()).expect("Memory registration failed!");
    verbs::post_send(&ctx, &id, 0, send_msg.as_bytes(), &send_mr, send_flags)
        .expect("Connect failed!");

    let wc_send = verbs::get_send_comp(&ctx, &id).expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);
    let wc_recv = verbs::get_recv_comp(&ctx, &id).expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    println!("{:?}", recv_msg);

    assert_eq!(
        &recv_msg[..send_msg.len()],
        "Hello koala client!".as_bytes()
    );
    Ok(())
}
