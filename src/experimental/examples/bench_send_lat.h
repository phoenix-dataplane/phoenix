#include "bench_util.h"

int run_send_lat_client(Context *ctx)
{
    struct ibv_mr *recv_mr = NULL, *send_mr = NULL;
    int send_flags = 0, ret;
    uint64_t times[ctx->num + 1];

    char recv_msg[ctx->size];
    char send_msg[ctx->size];

    ret = set_params(ctx);
    error_handler(ret, "rdma_getaddrinfo", out);
    if (ctx->attr.cap.max_inline_data >= ctx->size)
        send_flags |= IBV_SEND_INLINE;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    recv_mr = rdma_reg_msgs(ctx->id, recv_msg, ctx->size);
    error_handler_ret(!recv_mr, "rdma_reg_msgs for recv_msg", -1, out_destroy_ep);

    send_mr = rdma_reg_msgs(ctx->id, send_msg, ctx->size);
    error_handler_ret(!send_mr, "rdma_reg_msgs for send_msg", -1, out_destroy_ep);

    for (int i = 0; i < ctx->num; i++)
    {
        ret = rdma_post_recv(ctx->id, NULL, recv_msg, ctx->size, recv_mr);
        error_handler(ret, "rdma_post_recv", out_destroy_ep);
    }

    ret = rdma_connect(ctx->id, NULL);
    error_handler(ret, "rdma_connect", out_destroy_ep);

    struct ibv_wc wc;
    for (int i = 0; i < ctx->num; i++)
    {
        // if (i == ctx->warmup)
        //     // t1 = get_timestamp_us();
        //     t1 = get_cycles();
        times[i] = get_cycles();

        if (i == ctx->num - 1)
            send_flags |= IBV_SEND_SIGNALED;
        ret = rdma_post_send(ctx->id, NULL, send_msg, ctx->size, send_mr, send_flags);
        error_handler(ret, "rdma_post_send", out_disconnect);

        while (ibv_poll_cq(ctx->id->recv_cq, 1, &wc) == 0)
            ;
        error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                          out_disconnect);
    }
    while (ibv_poll_cq(ctx->id->send_cq, 1, &wc) == 0)
        ;
    error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                      out_disconnect);
    // t2 = get_timestamp_us();
    // printf("sum: %ld, avg delay: %.2lf\n", (t2 - t1) / 2, 1.0 * (t2 - t1) / ctx->num / 2);

    times[ctx->num] = get_cycles();
    print_lat(times, ctx->num + 1, ctx->warmup);

    // double factor = 2 * get_cpu_mhz(1);
    // printf("sum: %.2lf, avg delay: %.2lf\n", (t2 - t1) / factor, 1.0 * (t2 - t1) / ctx->num / factor);

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_ep:
    rdma_destroy_ep(ctx->id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
out:
    if (send_mr)
        rdma_dereg_mr(send_mr);
    if (recv_mr)
        rdma_dereg_mr(recv_mr);
    return ret;
}

int run_send_lat_server(Context *ctx)
{
    struct ibv_mr *recv_mr = NULL, *send_mr = NULL;
    struct ibv_wc *wcs;
    int send_flags = 0, ret;
    // uint64_t t1, t2;

    char recv_msg[ctx->size];
    char send_msg[ctx->size];

    ctx->ip = "0.0.0.0";
    ret = set_params(ctx);
    error_handler(ret, "rdma_getaddrinfo", out);
    if (ctx->attr.cap.max_inline_data >= ctx->size)
        send_flags |= IBV_SEND_INLINE;

    ret = rdma_create_ep(&ctx->listen_id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    ret = rdma_listen(ctx->listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep);

    ret = rdma_get_request(ctx->listen_id, &ctx->id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep);

    recv_mr = rdma_reg_msgs(ctx->id, recv_msg, ctx->size);
    error_handler_ret(!recv_mr, "rdma_reg_msgs for recv_msg", -1, out_destroy_accept_ep);

    send_mr = rdma_reg_msgs(ctx->id, send_msg, ctx->size);
    error_handler_ret(!send_mr, "rdma_reg_msgs for send_msg", -1, out_destroy_accept_ep);

    for (int i = 0; i < ctx->num; i++)
    {
        ret = rdma_post_recv(ctx->id, NULL, recv_msg, ctx->size, recv_mr);
        error_handler(ret, "rdma_post_recv", out_destroy_accept_ep);
    }

    ret = rdma_accept(ctx->id, NULL);
    error_handler(ret, "rdma_accpet", out_destroy_accept_ep);

    struct ibv_wc wc;
    for (int i = 0; i < ctx->num; i++)
    {
        while (ibv_poll_cq(ctx->id->recv_cq, 1, &wc) == 0)
            ;
        error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                          out_disconnect);

        if (i == ctx->num - 1)
            send_flags |= IBV_SEND_SIGNALED;
        ret = rdma_post_send(ctx->id, NULL, send_msg, ctx->size, send_mr, send_flags);
        error_handler(ret, "rdma_post_send", out_disconnect);
    }
    while (ibv_poll_cq(ctx->id->send_cq, 1, &wc) == 0)
        ;
    error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1,
                      out_disconnect);

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_accept_ep:
    rdma_destroy_ep(ctx->id);
out_destroy_listen_ep:
    rdma_destroy_ep(ctx->listen_id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
out:
    if (send_mr)
        rdma_dereg_mr(send_mr);
    if (recv_mr)
        rdma_dereg_mr(recv_mr);
    return ret;
};
