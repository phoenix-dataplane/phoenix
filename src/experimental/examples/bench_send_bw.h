#include "bench_util.h"

int run_send_bw_client(Context *ctx)
{
    struct ibv_mr *send_mr = NULL;
    int send_flags = 0, ret;
    int scnt = 0, ccnt = 0;
    uint64_t tposted[ctx->num + 1], tcompleted[ctx->num + 1];

    char send_msg[ctx->size];

    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    send_mr = rdma_reg_msgs(ctx->id, send_msg, ctx->size);
    error_handler_ret(!send_mr, "rdma_reg_msgs for send_msg", -1, out_destroy_ep);

    ret = rdma_connect(ctx->id, NULL);
    error_handler(ret, "rdma_connect", out_destroy_ep);

    struct ibv_wc wc[CTX_POLL_BATCH];
    while (scnt < ctx->num || ccnt < ctx->num)
    {
        if (scnt < ctx->num)
        {
            tposted[scnt] = get_cycles();
            ret = rdma_post_send(ctx->id, NULL, send_msg, ctx->size, send_mr, send_flags);
            error_handler(ret, "rdma_post_send", out_disconnect);
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

    print_bw(ctx, tposted, tcompleted);

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_ep:
    rdma_destroy_ep(ctx->id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
    if (send_mr)
        rdma_dereg_mr(send_mr);
    return ret;
}

int run_send_bw_server(Context *ctx)
{
    struct ibv_mr *recv_mr = NULL;
    int ret;

    char recv_msg[ctx->size];

    ret = rdma_create_ep(&ctx->listen_id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    ret = rdma_listen(ctx->listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep);

    ret = rdma_get_request(ctx->listen_id, &ctx->id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep);

    recv_mr = rdma_reg_msgs(ctx->id, recv_msg, ctx->size);
    error_handler_ret(!recv_mr, "rdma_reg_msgs for recv_msg", -1, out_destroy_accept_ep);

    for (int i = 0; i < ctx->num; i++)
    {
        ret = rdma_post_recv(ctx->id, NULL, recv_msg, ctx->size, recv_mr);
        error_handler(ret, "rdma_post_recv", out_destroy_accept_ep);
    }

    ret = rdma_accept(ctx->id, NULL);
    error_handler(ret, "rdma_accpet", out_destroy_accept_ep);

    struct ibv_wc wc[CTX_POLL_BATCH];
    for (int ccnt = 0; ccnt < ctx->num;)
    {
        int ne = ibv_poll_cq(ctx->id->recv_cq, CTX_POLL_BATCH, wc);
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

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_accept_ep:
    rdma_destroy_ep(ctx->id);
out_destroy_listen_ep:
    rdma_destroy_ep(ctx->listen_id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
    if (recv_mr)
        rdma_dereg_mr(recv_mr);
    return ret;
};
