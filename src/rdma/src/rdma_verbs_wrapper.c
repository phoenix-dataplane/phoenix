#include "rdma_verbs_wrapper.h"

int rdma_seterrno_real(int ret) {
        return rdma_seterrno(ret);
}

struct ibv_mr *rdma_reg_msgs_real(struct rdma_cm_id *id, void *addr, size_t length) {
        return rdma_reg_msgs(id, addr, length);
}

struct ibv_mr *rdma_reg_read_real(struct rdma_cm_id *id, void *addr, size_t length) {
        return rdma_reg_read(id, addr, length);
}

struct ibv_mr *rdma_reg_write_real(struct rdma_cm_id *id, void *addr, size_t length) {
        return rdma_reg_write(id, addr, length);
}

struct sockaddr *rdma_get_local_addr_real(struct rdma_cm_id *id)
{
	return rdma_get_local_addr(id);
}

struct sockaddr *rdma_get_peer_addr_real(struct rdma_cm_id *id)
{
	return rdma_get_peer_addr(id);
}

int rdma_dereg_mr_real(struct ibv_mr *mr) {
        return rdma_dereg_mr(mr);
}

int rdma_post_recvv_real(struct rdma_cm_id *id, void *context, struct ibv_sge *sgl, int nsge) {
        return rdma_post_recvv(id, context, sgl, nsge);
}

int rdma_post_sendv_real(struct rdma_cm_id *id, void *context, struct ibv_sge *sgl, int nsge, int flags) {
        return rdma_post_sendv(id, context, sgl, nsge, flags);
}

int rdma_post_readv_real(struct rdma_cm_id *id, void *context, struct ibv_sge *sgl, int nsge, int flags, uint64_t remote_addr, uint32_t rkey) {
        return rdma_post_readv(id, context, sgl, nsge, flags, remote_addr, rkey);
}

int rdma_post_writev_real(struct rdma_cm_id *id, void *context, struct ibv_sge *sgl, int nsge, int flags, uint64_t remote_addr, uint32_t rkey) {
        return rdma_post_writev(id, context, sgl, nsge, flags, remote_addr, rkey);
}

int rdma_post_recv_real(struct rdma_cm_id *id, void *context, void *addr, size_t length, struct ibv_mr *mr) {
        return rdma_post_recv(id, context, addr, length, mr);
}

int rdma_post_send_real(struct rdma_cm_id *id, void *context, void *addr, size_t length, struct ibv_mr *mr, int flags) {
        return rdma_post_send(id, context, addr, length, mr, flags);
}

int rdma_post_read_real(struct rdma_cm_id *id, void *context, void *addr, size_t length, struct ibv_mr *mr, int flags, uint64_t remote_addr, uint32_t rkey) {
        return rdma_post_read(id, context, addr, length, mr, flags, remote_addr, rkey);
}

int rdma_post_write_real(struct rdma_cm_id *id, void *context, void *addr, size_t length, struct ibv_mr *mr, int flags, uint64_t remote_addr, uint32_t rkey) {
        return rdma_post_write(id, context, addr, length, mr, flags, remote_addr, rkey);
}

int rdma_post_ud_send_real(struct rdma_cm_id *id, void *context, void *addr, size_t length, struct ibv_mr *mr, int flags, struct ibv_ah *ah, uint32_t remote_qpn) {
        return rdma_post_ud_send(id, context, addr, length, mr, flags, ah, remote_qpn);
}

int rdma_get_send_comp_real(struct rdma_cm_id *id, struct ibv_wc *wc) {
        return rdma_get_send_comp(id, wc);
}

int rdma_get_recv_comp_real(struct rdma_cm_id *id, struct ibv_wc *wc) {
        return rdma_get_recv_comp(id, wc);
}