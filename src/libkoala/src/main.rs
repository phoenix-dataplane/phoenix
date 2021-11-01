use interface::{QpCapability, QpInitAttr, QpType};
use libkoala::*;

use dns_lookup::{AddrInfoHints, SockType};
use libc::{AI_ADDRCONFIG, AI_V4MAPPED};

const SERVER_ADDR: &str = "127.0.0.1";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = koala_register().expect("register failed!");

    let flags = AI_ADDRCONFIG | AI_V4MAPPED;
    let hints = AddrInfoHints {
        flags,
        socktype: SockType::Stream.into(),
        ..Default::default()
    };
    let ai = dns_lookup::getaddrinfo(
        Some(SERVER_ADDR),
        Some(&SERVER_PORT.to_string()),
        Some(hints),
    )
    .expect("getaddrinfo");

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

    let id = cm::koala_create_ep(&ctx, ai, None, Some(&qp_init_attr))?;

    let recv_msg = [0; 4096];
    let mr = cm::koala_reg_msgs(&ctx, &id, &recv_msg.as_ptr_range())
        .expect("Memory registration failed!");

    cm::koala_post_recv(&ctx, &id, 0, &recv_msg.as_ptr_range(), &mr).expect("Post recv failed!");

    cm::koala_connect(&ctx, &id, None).expect("Connect failed!");

    let send_flags = 0;
    let send_msg = "Hello koala end!";
    cm::koala_post_send(
        &ctx,
        &id,
        0,
        &send_msg.as_bytes().as_ptr_range(),
        &mr,
        send_flags,
    )
    .expect("Connect failed!");

    let wc_send = cm::koala_get_send_comp(&ctx, &id).expect("Get send comp failed!");
    let wc_recv = cm::koala_get_recv_comp(&ctx, &id).expect("Get recv comp failed!");

    Ok(())
}
