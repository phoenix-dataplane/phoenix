#include "bench_util.h"

int run_read_bw_client(Context *ctx)
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
    volatile int *post_buf = (volatile int *)write_msg;

    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    read_mr = rdma_reg_msgs(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_msgs for read_msg", -1, out_destroy_ep);

    write_mr = rdma_reg_read(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_read for write_msg", -1, out_destroy_ep);

    ret = handshake(ctx, write_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_ep);
    printf("handshake finished\n");

    struct ibv_wc wc[CTX_POLL_BATCH];
    while (scnt < ctx->num || ccnt < ctx->num)
    {
        if (scnt < ctx->num && scnt - ccnt < ctx->attr.cap.max_send_wr)
        {
            tposted[scnt] = get_cycles();
            ret = rdma_post_read(ctx->id, NULL, read_msg, ctx->size, read_mr, send_flags,
                                 (uint64_t)remote_mr.addr, remote_mr.rkey);
            error_handler(ret, "rdma_post_read", out_disconnect);
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
    *post_buf = ctx->num; // let server know that read finished

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

int run_read_bw_server(Context *ctx)
{
    struct ibv_mr *read_mr, *write_mr, remote_mr;
    int send_flags = 0, ret;
    int scnt = 0;

    char *write_msg = ctx->send_buf;
    char *read_msg = ctx->recv_buf;
    memset(read_msg, 0, ctx->size);
    volatile int *poll_buf = (volatile int *)read_msg;

    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->listen_id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    ret = rdma_listen(ctx->listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep);

    ret = rdma_get_request(ctx->listen_id, &ctx->id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep);

    read_mr = rdma_reg_msgs(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_msgs for read_msg", -1, out_destroy_accept_ep);

    write_mr = rdma_reg_read(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_read for write_msg", -1, out_destroy_accept_ep);

    ret = handshake(ctx, write_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_accept_ep);
    printf("handshake finished\n");

    struct ibv_wc wc;
    while (*poll_buf != ctx->num)
    {
        ret = rdma_post_read(ctx->id, NULL, read_msg, ctx->size, read_mr, send_flags,
                             (uint64_t)remote_mr.addr, remote_mr.rkey);
        error_handler(ret, "rdma_post_read", out_disconnect);
        ret = poll_cq_and_check(ctx->id->send_cq, 1, &wc);
        error_handler(ret, "poll_cq", out_disconnect);
        scnt += 1;
    }

    /*
    struct ibv_wc wc[CTX_POLL_BATCH];
    while (read_msg[0] != 1 || ccnt < scnt)
    {
        if (read_msg[0] != 1 && scnt - ccnt < ctx->attr.cap.max_send_wr)
        {
            ret = rdma_post_read(ctx->id, NULL, read_msg, ctx->size, read_mr, send_flags,
    (uint64_t)remote_mr.addr, remote_mr.rkey); error_handler(ret, "rdma_post_read", out_disconnect);
            scnt += 1;
        }
        if (ccnt < scnt)
        {
            int ne = ibv_poll_cq(ctx->id->send_cq, CTX_POLL_BATCH, wc);
            if (ne > 0)
            {
                for (int i = 0; i < ne; i++)
                {
                    error_handler_ret(wc[i].status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                                      out_disconnect);
                    ccnt += 1;
                }
            }
        }
    }
    */

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_accept_ep:
    rdma_destroy_ep(ctx->id);
out_destroy_listen_ep:
    rdma_destroy_ep(ctx->listen_id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
    if (read_mr)
        rdma_dereg_mr(read_mr);
    if (write_mr)
        rdma_dereg_mr(write_mr);
    return ret;
};
