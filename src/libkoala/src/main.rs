use ibverbs::ffi::*;
use libkoala::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    koala_register().expect("register failed");

    ibv_context;
    let hints = rdma_cm_id {
        ai_port_space: RDMA_PS_TCP,
    };
    let addr = rdma_getaddrinfo(server, port, &hints).expect("getaddrinfo failed");

    let attr = ibv_qp_init_attr::new();
    attr.cap.max_send_wr = 1;
    attr.cap.max_recv_wr = 1;
    attr.cap.max_send_sge = 1;
    attr.cap.max_recv_sge = 1;
    attr.cap.max_inline_data = 16;
    attr.qp_context = id; //???
    attr.sq_sig_all = 1;

    let id = rdma_create_ep(addr, None, attr);
    let recv_mr = rdma_reg_msgs(id, recv_msg).expect("register memory failed");
    let send_mr = rdma_reg_msgs(id, send_msg).expect("register memory failed");
    rdma_connect(id, None).expect("rdma connect failed");
    let send_flags = i32;
    rdma_post_send(id, None, send_msg, send_mr, send_flags).expect("rdma post send failed");
    rdma_get_send_comp().expect("rdma get send comp failed");
    rdma_get_recv_comp().expect("rdma get recv comp failed");

    Ok(())
}
