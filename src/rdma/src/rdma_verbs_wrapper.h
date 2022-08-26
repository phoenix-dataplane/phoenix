#ifndef RDMA_VERBS_WRAPPER_H

#include <rdma/rdma_verbs.h>

#ifdef __cplusplus
extern "C" {
#endif

int rdma_seterrno_real(int ret);

struct ibv_mr* rdma_reg_msgs_real(struct rdma_cm_id* id, void* addr,
                                  size_t length);

struct ibv_mr* rdma_reg_read_real(struct rdma_cm_id* id, void* addr,
                                  size_t length);

struct ibv_mr* rdma_reg_write_real(struct rdma_cm_id* id, void* addr,
                                   size_t length);

struct sockaddr* rdma_get_local_addr_real(struct rdma_cm_id *id);

struct sockaddr* rdma_get_peer_addr_real(struct rdma_cm_id *id);

int rdma_dereg_mr_real(struct ibv_mr* mr);

int rdma_post_recvv_real(struct rdma_cm_id* id, void* context,
                         struct ibv_sge* sgl, int nsge);

int rdma_post_sendv_real(struct rdma_cm_id* id, void* context,
                         struct ibv_sge* sgl, int nsge, int flags);

int rdma_post_readv_real(struct rdma_cm_id* id, void* context,
                         struct ibv_sge* sgl, int nsge, int flags,
                         uint64_t remote_addr, uint32_t rkey);

int rdma_post_writev_real(struct rdma_cm_id* id, void* context,
                          struct ibv_sge* sgl, int nsge, int flags,
                          uint64_t remote_addr, uint32_t rkey);

int rdma_post_recv_real(struct rdma_cm_id* id, void* context, void* addr,
                        size_t length, struct ibv_mr* mr);

int rdma_post_send_real(struct rdma_cm_id* id, void* context, void* addr,
                        size_t length, struct ibv_mr* mr, int flags);

int rdma_post_read_real(struct rdma_cm_id* id, void* context, void* addr,
                        size_t length, struct ibv_mr* mr, int flags,
                        uint64_t remote_addr, uint32_t rkey);

int rdma_post_write_real(struct rdma_cm_id* id, void* context, void* addr,
                         size_t length, struct ibv_mr* mr, int flags,
                         uint64_t remote_addr, uint32_t rkey);

int rdma_post_ud_send_real(struct rdma_cm_id* id, void* context, void* addr,
                           size_t length, struct ibv_mr* mr, int flags,
                           struct ibv_ah* ah, uint32_t remote_qpn);

int rdma_get_send_comp_real(struct rdma_cm_id* id, struct ibv_wc* wc);

int rdma_get_recv_comp_real(struct rdma_cm_id* id, struct ibv_wc* wc);

#ifdef __cplusplus
}
#endif

#endif  // RDMA_VERBS_WRAPPER_H
