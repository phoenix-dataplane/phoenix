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
