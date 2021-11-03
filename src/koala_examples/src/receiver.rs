use interface::{
    addrinfo::{AddrFamily, AddrInfoFlags, AddrInfoHints, PortSpace},
    QpCapability, QpInitAttr, QpType,
};
use libkoala::{cm, verbs, koala_register};

const SERVER_ADDR: &str = "0.0.0.0";
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

    let recv_msg = [0u8; 128];
    let mr =
        cm::reg_msgs(&ctx, &id, &recv_msg).expect("Memory registration failed!");

    verbs::post_recv(&ctx, &id, 0, &recv_msg, &mr).expect("Post recv failed!");

    cm::accept(&ctx, &id, None).expect("Accept failed!");

    let wc_recv = verbs::get_recv_comp(&ctx, &id).expect("Get recv comp failed!");

    let send_flags = Default::default();
    let send_msg = "Hello koala client!";
    verbs::post_send(
        &ctx,
        &id,
        0,
        &send_msg.as_bytes(),
        &mr,
        send_flags,
    )
    .expect("Connect failed!");

    let wc_send = verbs::get_send_comp(&ctx, &id).expect("Get send comp failed!");

    println!("{:#?}", recv_msg);
    Ok(())
}
