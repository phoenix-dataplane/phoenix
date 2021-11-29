#include "bench_util.h"

int run_write_bw_client(Context *ctx)
{
    struct ibv_mr *read_mr = NULL, *write_mr = NULL, remote_mr;
    int send_flags = 0, ret;
    uint32_t scnt = 0, ccnt = 0;

    uint64_t *tposted = ctx->times1_buf;
    uint64_t *tcompleted = ctx->times2_buf;
    char *write_msg = ctx->send_buf;
    char *read_msg = ctx->recv_buf;
    memset(write_msg, 0, ctx->size);
    memset(read_msg, 255, ctx->size);
    volatile uint32_t *post_buf = (volatile uint32_t *)write_msg;

    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    read_mr = rdma_reg_write(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_write for read_msg", -1, out_destroy_ep);

    write_mr = rdma_reg_msgs(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_msgs for write_msg", -1, out_destroy_ep);

    ret = handshake(ctx, read_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_ep);
    printf("handshake finished\n");

    struct ibv_wc wc[CTX_POLL_BATCH];
    while (scnt < ctx->num || ccnt < ctx->num)
    {
        if (scnt < ctx->num && scnt - ccnt < ctx->attr.cap.max_send_wr)
        {
            tposted[scnt] = get_cycles();
            *post_buf = scnt;
            ret = rdma_post_write(ctx->id, NULL, write_msg, ctx->size, write_mr, send_flags,
                                  (uint64_t)remote_mr.addr, remote_mr.rkey);
            error_handler(ret, "rdma_post_write", out_disconnect);
            scnt += 1;
        }
        if (ccnt < ctx->num)
        {
            int ne = ibv_poll_cq(ctx->id->send_cq, CTX_POLL_BATCH, wc);
            if (ne > 0)
            {
                for (int i = 0; i < ne; i++)
                {
                    error_handler_ret(wc[i].status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                                      out_disconnect);
                    ccnt += 1;
                    tcompleted[ccnt] = get_cycles();
                }
            }
        }
    }
    *post_buf = ctx->num;
    ret = rdma_post_write(ctx->id, NULL, write_msg, ctx->size, write_mr,
                          send_flags & (~IBV_SEND_SIGNALED), (uint64_t)remote_mr.addr,
                          remote_mr.rkey);
    error_handler(ret, "rdma_post_write", out_disconnect);

    print_bw(ctx, tposted, tcompleted);

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_ep:
    rdma_destroy_ep(ctx->id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
    if (read_mr)
        rdma_dereg_mr(read_mr);
    if (write_mr)
        rdma_dereg_mr(write_mr);
    return ret;
}

int run_write_bw_server(Context *ctx)
{
    struct ibv_mr *read_mr, remote_mr;
    int ret;

    char *read_msg = ctx->recv_buf;
    memset(read_msg, 255, ctx->size);
    volatile uint32_t *poll_buf = (volatile uint32_t *)read_msg;

    ret = rdma_create_ep(&ctx->listen_id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    ret = rdma_listen(ctx->listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep);

    ret = rdma_get_request(ctx->listen_id, &ctx->id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep);

    read_mr = rdma_reg_write(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_write for read_msg", -1, out_destroy_accept_ep);

    // write_mr = rdma_reg_msgs(ctx->id, write_msg, ctx->size);
    // error_handler_ret(!write_mr, "rdma_reg_msgs for write_msg", -1, out_destroy_ep);

    ret = handshake(ctx, read_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_accept_ep);
    printf("handshake finished\n");

    while (*poll_buf != ctx->num)
        ;

    rdma_disconnect(ctx->id);
out_destroy_accept_ep:
    rdma_destroy_ep(ctx->id);
out_destroy_listen_ep:
    rdma_destroy_ep(ctx->listen_id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
    if (read_mr)
        rdma_dereg_mr(read_mr);
    // if (write_mr)
    //     rdma_dereg_mr(write_mr);
    return ret;
};
