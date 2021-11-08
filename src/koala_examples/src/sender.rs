use libkoala::{cm, Context, verbs};
use libkoala::verbs::{SendFlags, WcStatus};

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::register().expect("register failed");

    let ai = cm::getaddrinfo(
        &ctx,
        Some(&SERVER_ADDR),
        Some(&SERVER_PORT.to_string()),
        None,
    )
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

    let id = cm::CmId::create_ep(ctx, &ai, None, Some(&qp_init_attr))?;

    eprintln!("cmid created");

    let mut recv_msg = vec![0u8; 128];
    let recv_mr = id.reg_msgs(&recv_msg).expect("Memory registration failed!");

    unsafe {
        // Should I force the post_send/recv to take an memory region (or a slice of memory region)
        // as its input to make sure the memory is registered?
        id.post_recv(0, &mut recv_msg, &recv_mr).expect("Post recv failed!");
    }

    id.connect(None).expect("Connect failed!");

    let send_flags = SendFlags::SIGNALED;
    let send_msg = "Hello koala server!";
    let send_mr = id.reg_msgs(send_msg.as_bytes()).expect("Memory registration failed!");
    id.post_send(0, send_msg.as_bytes(), &send_mr, send_flags)
        .expect("Connect failed!");

    let wc_send = id.get_send_comp().expect("Get send comp failed!");
    assert_eq!(wc_send.status, WcStatus::Success);
    let wc_recv = id.get_recv_comp().expect("Get recv comp failed!");
    assert_eq!(wc_recv.status, WcStatus::Success);

    println!("{:?}", recv_msg);

    assert_eq!(
        &recv_msg[..send_msg.len()],
        "Hello koala client!".as_bytes()
    );
    Ok(())
}
