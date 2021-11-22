#include "bench_util.h"

int run_read_lat_client(Context *ctx)
{
    struct ibv_mr *read_mr = NULL, *write_mr = NULL, remote_mr;
    int send_flags = 0, ret = 0;
    uint64_t times[ctx->num + 1];

    char write_msg[ctx->size];
    char read_msg[ctx->size];
    memset(read_msg, 255, ctx->size);
    // volatile int *post_buf = (volatile int *)write_msg;

    send_flags |= IBV_SEND_SIGNALED;

    ret = rdma_create_ep(&ctx->id, ctx->ai, NULL, &ctx->attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    read_mr = rdma_reg_msgs(ctx->id, read_msg, ctx->size);
    error_handler_ret(!read_mr, "rdma_reg_msgs for read_msg", -1, out_destroy_ep);

    write_mr = rdma_reg_read(ctx->id, write_msg, ctx->size);
    error_handler_ret(!write_mr, "rdma_reg_read for send_msg", -1, out_destroy_ep);

    ret = handshake(ctx, write_mr, &remote_mr);
    error_handler(ret, "handshake", out_destroy_ep);
    printf("handshake finished\n");

    struct ibv_wc wc;
    for (int i = 0; i < ctx->num; i++)
    {
        times[i] = get_cycles();

        // *post_buf = i;
        ret = rdma_post_read(ctx->id, NULL, read_msg, ctx->size, read_mr, send_flags,
                             (uint64_t)remote_mr.addr, remote_mr.rkey);
        error_handler(ret, "rdma_post_read", out_disconnect);
        while (ibv_poll_cq(ctx->id->send_cq, 1, &wc) == 0)
            ;
        error_handler_ret(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq", -1, out_disconnect);
    }
    times[ctx->num] = get_cycles();
    print_lat(ctx, times);

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

int run_read_lat_server(Context *ctx)
{
    struct ibv_mr *read_mr, *write_mr, remote_mr;
    int send_flags = 0, ret;

    char write_msg[ctx->size];
    char read_msg[ctx->size];
    memset(read_msg, 255, ctx->size);
    // volatile int *post_buf = (volatile int *)write_msg;

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
    for (int i = 0; i < ctx->num; i++)
    {
        // *post_buf = i;
        ret = rdma_post_read(ctx->id, NULL, read_msg, ctx->size, read_mr, send_flags,
                             (uint64_t)remote_mr.addr, remote_mr.rkey);
        error_handler(ret, "rdma_post_read", out_disconnect);
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
    if (read_mr)
        rdma_dereg_mr(read_mr);
    if (write_mr)
        rdma_dereg_mr(write_mr);
    return ret;
};
