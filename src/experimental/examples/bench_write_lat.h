#include "bench_util.h"

int handshake(Context *ctx, struct ibv_mr *read_mr, struct ibv_mr *remote_mr)
{
    struct ibv_mr *hsk_send_mr = NULL, *hsk_recv_mr = NULL;
    int ret = 0;

    hsk_send_mr = rdma_reg_msgs(ctx->id, read_mr, sizeof(struct ibv_mr));
    hsk_recv_mr = rdma_reg_msgs(ctx->id, remote_mr, sizeof(struct ibv_mr));

    ret = rdma_post_recv(ctx->id, NULL, remote_mr, sizeof(struct ibv_mr), hsk_recv_mr);
    error_handler(ret, "rdma_post_recv", out);

    if (ctx->client)
    {
        ret = rdma_connect(ctx->id, NULL);
        error_handler(ret, "rdma_connect", out);
    }
    else
    {
        ret = rdma_accept(ctx->id, NULL);
        error_handler(ret, "rdma_accpet", out);
    }

    ret = rdma_post_send(ctx->id, NULL, read_mr, sizeof(struct ibv_mr), hsk_send_mr, IBV_SEND_INLINE | IBV_SEND_SIGNALED);
    error_handler(ret, "rdma_post_send", out);

    struct ibv_wc wc;
    rdma_get_send_comp(ctx->id, &wc);
    error_handler_ret(wc.status != IBV_WC_SUCCESS, "rdma_get_send_comp", -1, out);

    rdma_get_recv_comp(ctx->id, &wc);
    error_handler_ret(wc.status != IBV_WC_SUCCESS, "rdma_get_recv_comp", -1, out);

out:
    if (hsk_send_mr)
        rdma_dereg_mr(hsk_send_mr);
    if (hsk_recv_mr)
        rdma_dereg_mr(hsk_recv_mr);
    return ret;
}

int run_write_lat_client(Context *ctx)
{
    struct ibv_mr *read_mr = NULL, *write_mr = NULL, remote_mr;
    int send_flags = 0, ret = 0;
    uint64_t t1, t2;

    char write_msg[ctx->size];
    char read_msg[ctx->size];
    memset(read_msg, 255, ctx->size);
    volatile int *post_buf = (volatile int *)write_msg;
    volatile int *poll_buf = (volatile int *)read_msg;

    ret = set_params(ctx);
    error_handler(ret, "rdma_getaddrinfo", out);
    if (ctx->attr.cap.max_inline_data >= ctx->size)
        send_flags |= IBV_SEND_INLINE;
    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    read_mr = rdma_reg_write(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_write for read_msg", -1, out_destroy_ep);

    write_mr = rdma_reg_write(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_write for send_msg", -1, out_destroy_ep);

    ret = handshake(ctx, read_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_ep);
    printf("handshake finished\n");

    struct ibv_wc wc;
    t1 = get_timestamp_us();
    for (int i = 0; i < ctx->num; i++)
    {
        *post_buf = i;
        ret = rdma_post_write(ctx->id, NULL, write_msg, ctx->size, write_mr, send_flags, (uint64_t)remote_mr.addr, remote_mr.rkey);
        error_handler(ret, "rdma_post_write", out_disconnect);
        while (ibv_poll_cq(ctx->id->send_cq, 1, &wc) == 0)
            ;
        error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1, out_disconnect);
        while (*poll_buf != i)
            ;
    }
    t2 = get_timestamp_us();
    printf("sum: %ld, avg delay: %.2lf\n", (t2 - t1) / 2, 1.0 * (t2 - t1) / ctx->num / 2);

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_ep:
    rdma_destroy_ep(ctx->id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
out:
    if (read_mr)
        rdma_dereg_mr(read_mr);
    if (write_mr)
        rdma_dereg_mr(write_mr);
    return ret;
}

int run_write_lat_server(Context *ctx)
{
    struct ibv_mr *read_mr, *write_mr, remote_mr;
    struct ibv_wc *wcs;
    int send_flags = 0, ret;
    // uint64_t t1, t2;

    char write_msg[ctx->size];
    char read_msg[ctx->size];
    memset(read_msg, 255, ctx->size);
    volatile int *post_buf = (volatile int *)write_msg;
    volatile int *poll_buf = (volatile int *)read_msg;

    ctx->ip = "0.0.0.0";
    ret = set_params(ctx);
    error_handler(ret, "rdma_getaddrinfo", out);
    if (ctx->attr.cap.max_inline_data >= ctx->size)
        send_flags |= IBV_SEND_INLINE;
    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->listen_id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    ret = rdma_listen(ctx->listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep);

    ret = rdma_get_request(ctx->listen_id, &ctx->id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep);

    read_mr = rdma_reg_write(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_write for read_msg", -1, out_destroy_accept_ep);

    write_mr = rdma_reg_write(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_write for write_msg", -1, out_destroy_accept_ep);

    ret = handshake(ctx, read_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_accept_ep);
    printf("handshake finished\n");

    struct ibv_wc wc;
    for (int i = 0; i < ctx->num; i++)
    {
        while (*poll_buf != i)
            ;
        *post_buf = i;
        ret = rdma_post_write(ctx->id, NULL, write_msg, ctx->size, write_mr, send_flags, (uint64_t)remote_mr.addr, remote_mr.rkey);
        error_handler(ret, "rdma_post_write", out_disconnect);
        while (ibv_poll_cq(ctx->id->send_cq, 1, &wc) == 0)
            ;
        error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1, out_disconnect);
    }

out_disconnect:
    rdma_disconnect(ctx->id);
out_destroy_accept_ep:
    rdma_destroy_ep(ctx->id);
out_destroy_listen_ep:
    rdma_destroy_ep(ctx->listen_id);
out_free_addrinfo:
    rdma_freeaddrinfo(ctx->ai);
out:
    if (read_mr)
        rdma_dereg_mr(read_mr);
    if (write_mr)
        rdma_dereg_mr(write_mr);
    return ret;
};
