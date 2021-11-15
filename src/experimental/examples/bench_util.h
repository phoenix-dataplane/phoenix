#pragma once

#include <stdio.h>
#include <stdlib.h>
#include <sys/time.h>
#include <errno.h>
#include <rdma/rdma_cma.h>
#include <rdma/rdma_verbs.h>
#include "get_clock.h"

#define MAX(a, b) ((a) > (b)) ? (a) : (b);

#define error_handler_ret(cond, str, v, label) \
    if (cond)                                  \
    {                                          \
        perror(str);                           \
        ret = v;                               \
        goto label;                            \
    }

#define error_handler(cond, str, label) \
    if (cond)                           \
    {                                   \
        perror(str);                    \
        goto label;                     \
    }

uint64_t get_timestamp_us() //microseconds
{
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return tv.tv_sec * (uint64_t)1000000 + tv.tv_usec;
}

enum Operation
{
    SEND,
    WRITE,
    READ
};

typedef struct
{
    enum Operation opt;
    int size, num, warmup;
    char *ip, *port;
    bool client;
    struct rdma_cm_id *id, *listen_id;
    struct ibv_qp_init_attr attr;
    struct rdma_addrinfo *ai;
} Context;

int set_params(Context *ctx)
{
    struct rdma_addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_port_space = RDMA_PS_TCP;
    if (!ctx->client)
        hints.ai_flags = RAI_PASSIVE;
    int ret = rdma_getaddrinfo(ctx->ip, ctx->port, &hints, &ctx->ai);
    if (ret)
        return ret;

    struct ibv_qp_init_attr *attr = &ctx->attr;
    memset(attr, 0, sizeof(struct ibv_qp_init_attr));
    attr->cap.max_send_wr = attr->cap.max_recv_wr = 1024;
    attr->cap.max_send_sge = attr->cap.max_recv_sge = 1;
    attr->cap.max_inline_data = 236;
    attr->qp_context = ctx->id;
    attr->sq_sig_all = 0;
    return 0;
}

int handshake(Context *ctx, struct ibv_mr *mr, struct ibv_mr *remote_mr)
{
    struct ibv_mr *hsk_send_mr = NULL, *hsk_recv_mr = NULL;
    int ret = 0;

    hsk_send_mr = rdma_reg_msgs(ctx->id, mr, sizeof(struct ibv_mr));
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

    ret = rdma_post_send(ctx->id, NULL, mr, sizeof(struct ibv_mr), hsk_send_mr, IBV_SEND_INLINE | IBV_SEND_SIGNALED);
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

static int cmp(const void *a, const void *b)
{
    if (*(uint64_t *)a < *(uint64_t *)b)
        return -1;
    if (*(uint64_t *)a > *(uint64_t *)b)
        return 1;
    return 0;
}

#define LAT_MEASURE_TAIL (2)
void print_lat(Context *ctx, uint64_t times[])
{
    int num = ctx->num - ctx->warmup;
    uint64_t delta[num];
    for (int i = 0; i < num; i++)
        delta[i] = times[i + 1 + ctx->warmup] - times[i + ctx->warmup];

    double factor = get_cpu_mhz(1) * ((ctx->opt == READ) ? 1 : 2);
    qsort(delta, num, sizeof(uint64_t), cmp);

    int cnt = num - LAT_MEASURE_TAIL;
    double sum = 0;
    for (int i = 0; i < cnt; i++)
        sum += delta[i] / factor;
    printf("sum: %.2lf, avg: %.2lf\n", sum, sum / cnt);
}
