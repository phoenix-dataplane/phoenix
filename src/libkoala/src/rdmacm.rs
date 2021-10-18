use ibverbs::ffi::*;

enum ai_port_space {
    RDMA_PS_UDP,
    RDMA_PS_TCP,
    RDMA_PS_IB,
}

enum rdma_port_space {
    RDMA_PS_IPOIB = 0x0002,
    RDMA_PS_TCP = 0x0106,
    RDMA_PS_UDP = 0x0111,
    RDMA_PS_IB = 0x013F,
}

#[derive(Derivative)]
#[derivative(Default)]
pub struct rdma_addrinfo {
    #[derivative(Default(value = RDMA_PS_IB))]
    ai_port_space: ai_port_space,
}

struct rdma_event_channel {
    fd: i32,
}

pub struct rdma_cm_id {
    verbs: ibv_context,
    channel: rdma_event_channel,
    qp: ibv_qp,
    route: rdma_route,
    ps: rdma_port_space,
    port_num: u8,
}

struct rdma_conn_param {}

pub fn rdma_getaddrinfo(
    server: &str,
    port: u32,
    hint: &rdma_addrinfo,
) -> Result<rdma_addrinfo, Error> {
}

pub fn rdma_create_ep(
    res: rdma_addrinfo,
    pb: ibv_pb,
    attr: ibv_qp_init_attr,
) -> Result<rdma_cm_id, Error> {
}

pub fn rdma_reg_msgs<T>(id: &rdma_cm_id, msg: &T) -> Result<ibv_mr, Error> {}

pub fn rdma_connect(id: &rdma_cm_id, conn_param: Option<rdma_conn_param>) -> Result<Ok, Error> {}

pub fn rdma_post_send<T, U>(
    id: &rdma_cm_id,
    context: &Option<T>,
    msg: &U,
    mr: &ibv_mr,
    flags: i32,
) -> Result<Ok, Error> {
}
