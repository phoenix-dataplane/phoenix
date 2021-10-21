// use ibverbs::ffi::*;
use interface::{QpCapability, QpInitAttr, QpType};
use libkoala::*;

use dns_lookup::{AddrInfoHints, SockType};

const SERVER_ADDR: &str = "127.0.0.1";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = koala_register().expect("register failed");

    let hints = AddrInfoHints {
        socktype: SockType::Stream.into(),
        ..AddrInfoHints::default()
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

    // let ep = cm::koala_create_ep(&ctx, ai, None, None)?;
    let ep = cm::koala_create_ep(&ctx, ai, None, Some(&qp_init_attr))?;

    // ibv_context;
    // let hints = rdma_cm_id {
    //     ai_port_space: RDMA_PS_TCP,
    // };
    // let addr = rdma_getaddrinfo(server, port, &hints).expect("getaddrinfo failed");

    // let attr = ibv_qp_init_attr::new();
    // attr.cap.max_send_wr = 1;
    // attr.cap.max_recv_wr = 1;
    // attr.cap.max_send_sge = 1;
    // attr.cap.max_recv_sge = 1;
    // attr.cap.max_inline_data = 16;
    // attr.qp_context = id; //???
    // attr.sq_sig_all = 1;

    // let id = rdma_create_ep(addr, None, attr);
    // let recv_mr = rdma_reg_msgs(id, recv_msg).expect("register memory failed");
    // let send_mr = rdma_reg_msgs(id, send_msg).expect("register memory failed");
    // rdma_connect(id, None).expect("rdma connect failed");
    // let send_flags = i32;
    // rdma_post_send(id, None, send_msg, send_mr, send_flags).expect("rdma post send failed");
    // rdma_get_send_comp().expect("rdma get send comp failed");
    // rdma_get_recv_comp().expect("rdma get recv comp failed");

    Ok(())
}
