#include <errno.h>
#include <getopt.h>
#include <netdb.h>
#include <rdma/rdma_cma.h>
#include <rdma/rdma_verbs.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
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

int run_delay_client(int size, int num, char *srv_ip, char *srv_port)
{
    struct rdma_addrinfo hints, *res;
    struct ibv_qp_init_attr attr;
    struct ibv_mr *recv_mr, *send_mr;
    int send_flags = 0, ret;

    char recv_msg[size];
    char send_msg[size];

    memset(&hints, 0, sizeof hints);
    hints.ai_port_space = RDMA_PS_TCP;
    ret = rdma_getaddrinfo(srv_ip, srv_port, &hints, &res);
    error_handler(ret, "rdma_getaddrinfo", out);

    struct rdma_cm_id *id;
    memset(&attr, 0, sizeof attr);
    attr.cap.max_send_wr = attr.cap.max_recv_wr = 1024;
    attr.cap.max_send_sge = attr.cap.max_recv_sge = 1;
    attr.cap.max_inline_data = 16;
    attr.qp_context = id;
    attr.sq_sig_all = 0;
    ret = rdma_create_ep(&id, res, NULL, &attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo);

    recv_mr = rdma_reg_msgs(id, recv_msg, size);
    if (!recv_mr)
    {
        perror("rdma_reg_msgs for recv_msg");
        ret = -1;
        goto out_destroy_ep;
    }

    send_mr = rdma_reg_msgs(id, send_msg, size);
    if (!send_mr)
    {
        perror("rdma_reg_msgs for send_msg");
        ret = -1;
        goto out_dereg_recv;
    }

    for (int i = 0; i < num; i++)
    {
        ret = rdma_post_recv(id, NULL, recv_msg, size, recv_mr);
        error_handler(ret, "rdma_post_recv", out_dereg_send);
    }

    ret = rdma_connect(id, NULL);
    error_handler(ret, "rdma_connect", out_dereg_send);

    struct ibv_wc *wcs = (struct ibv_wc *)malloc(num * sizeof(struct ibv_wc));
    uint64_t t1 = get_timestamp_us();
    for (int i = 0; i < num; i++)
    {
        if (i == num - 1)
            send_flags |= IBV_SEND_SIGNALED;
        ret = rdma_post_send(id, NULL, send_msg, size, send_mr, send_flags);
        error_handler(ret, "rdma_post_send", out_disconnect);
        while (ibv_poll_cq(id->recv_cq, 1, wcs + i) == 0)
            ;
        error_handler(wcs[i].status != IBV_WC_SUCCESS, "ibv_poll_cq",
                      out_disconnect);
    }
    struct ibv_wc wc;
    while (ibv_poll_cq(id->send_cq, 1, &wc) == 0)
        ;
    error_handler(wc.status != IBV_WC_SUCCESS, "ibv_poll_cq",
                  out_disconnect);
    uint64_t t2 = get_timestamp_us();
    printf("sum: %ld, avg delay: %ld\n", t2 - t1, (t2 - t1) / num);
    /*
    uint64_t t1 = get_timestamp_us();
    for (int i = 0; i < num; i++)
    {
        ret = rdma_post_send(id, NULL, send_msg, size, send_mr, send_flags);
        error_handler(ret, "rdma_post_send", out_disconnect);
    }

    struct ibv_wc *wcs = (struct ibv_wc *)malloc(num * sizeof(struct ibv_wc));
    int cnt = 0;
    while (cnt < num)
    {
        int n = ibv_poll_cq(id->send_cq, num - cnt, wcs + cnt);
        for (int i = 0; i < n; i++)
            error_handler(wcs[i].status != IBV_WC_SUCCESS, "ibv_poll_cq",
                          out_disconnect);
        cnt += n;
    }
    uint64_t t2 = get_timestamp_us();
    
    cnt = 0;
    while (cnt < num)
    {
        int n = ibv_poll_cq(id->recv_cq, num - cnt, wcs + cnt);
        for (int i = 0; i < n; i++)
            error_handler(wcs[i].status != IBV_WC_SUCCESS, "ibv_poll_cq",
                          out_disconnect);
        cnt += n;
    }
    uint64_t t3 = get_timestamp_us();

    printf("%ld %ld %ld %ld\n", t2 - t1, (t2 - t1) / num, t3 - t1,
           (t3 - t1) / num);
    */

out_disconnect:
    free(wcs);
    rdma_disconnect(id);
out_dereg_send:
    rdma_dereg_mr(send_mr);
out_dereg_recv:
    rdma_dereg_mr(recv_mr);
out_destroy_ep:
    rdma_destroy_ep(id);
out_free_addrinfo:
    rdma_freeaddrinfo(res);
out:
    return ret;
}

int run_delay_server(int size, int num, char *srv_port)
{
    struct rdma_addrinfo hints, *res;
    struct ibv_qp_init_attr init_attr;
    struct ibv_mr *recv_mr, *send_mr;
    int send_flags = 0;
    int ret;

    char recv_msg[size];
    char send_msg[size];

    memset(&hints, 0, sizeof hints);
    hints.ai_flags = RAI_PASSIVE;
    hints.ai_port_space = RDMA_PS_TCP;
    ret = rdma_getaddrinfo("0.0.0.0", srv_port, &hints, &res);
    error_handler(ret, "rdma_getaddrinfo", out_srv);

    struct rdma_cm_id *listen_id, *id;
    memset(&init_attr, 0, sizeof init_attr);
    init_attr.cap.max_send_wr = init_attr.cap.max_recv_wr = 1024;
    init_attr.cap.max_send_sge = init_attr.cap.max_recv_sge = 1;
    init_attr.cap.max_inline_data = 16;
    init_attr.sq_sig_all = 0;
    ret = rdma_create_ep(&listen_id, res, NULL, &init_attr);
    error_handler(ret, "rdma_create_ep", out_free_addrinfo_srv);

    ret = rdma_listen(listen_id, 1);
    error_handler(ret, "rdma_listen", out_destroy_listen_ep_srv);

    ret = rdma_get_request(listen_id, &id);
    error_handler(ret, "rdma_get_request", out_destroy_listen_ep_srv);

    recv_mr = rdma_reg_msgs(id, recv_msg, size);
    if (!recv_mr)
    {
        ret = -1;
        perror("rdma_reg_msgs for recv_msg");
        goto out_destroy_accept_ep_srv;
    }

    send_mr = rdma_reg_msgs(id, send_msg, size);
    if (!send_mr)
    {
        ret = -1;
        perror("rdma_reg_msgs for send_msg");
        goto out_dereg_recv_srv;
    }

    for (int i = 0; i < num; i++)
    {
        ret = rdma_post_recv(id, NULL, recv_msg, size, recv_mr);
        error_handler(ret, "rdma_post_recv", out_dereg_send_srv);
    }

    ret = rdma_accept(id, NULL);
    error_handler(ret, "rdma_accpet", out_dereg_send_srv);

    struct ibv_wc *wcs = (struct ibv_wc *)malloc(num * sizeof(struct ibv_wc));
    /*
    int cnt = 0;
    while (cnt < num)
    {
        int n = ibv_poll_cq(id->recv_cq, num - cnt, wcs + cnt);
        for (int i = 0; i < n; i++)
            error_handler(wcs[i].status != IBV_WC_SUCCESS, "ibv_poll_cq",
                          out_disconnect_srv);
        cnt += n;
    }
    */

    for (int i = 0; i < num; i++)
    {
        while (ibv_poll_cq(id->recv_cq, 1, wcs + i) == 0)
            ;
        if (i == num - 1)
            send_flags |= IBV_SEND_SIGNALED;
        ret = rdma_post_send(id, NULL, send_msg, size, send_mr, send_flags);
        error_handler(ret, "rdma_post_send", out_disconnect_srv);
    }
    struct ibv_wc wc;
    while (ibv_poll_cq(id->send_cq, 1, &wc) == 0)
        ;

out_disconnect_srv:
    free(wcs);
    rdma_disconnect(id);
out_dereg_send_srv:
    rdma_dereg_mr(send_mr);
out_dereg_recv_srv:
    rdma_dereg_mr(recv_mr);
out_destroy_accept_ep_srv:
    rdma_destroy_ep(id);
out_destroy_listen_ep_srv:
    rdma_destroy_ep(listen_id);
out_free_addrinfo_srv:
    rdma_freeaddrinfo(res);
out_srv:
    return ret;
};

int main(int argc, char **argv)
{
    int op, num = 1000, size = 2;
    bool client = false;
    char *ip = "127.0.0.1", *port = "5000";

    while ((op = getopt(argc, argv, "c:p:n:s:")) != -1)
    {
        switch (op)
        {
        case 'c':
            client = true;
            ip = optarg;
            break;
        case 'p':
            port = optarg;
            break;
        case 'n':
            num = atoi(optarg);
            break;
        case 's':
            size = atoi(optarg);
            break;
        }
    }
    printf("num: %d, size: %d\n", num, size);
    int ret = 0;
    if (client)
        ret = run_delay_client(size, num, ip, port);
    else
        ret = run_delay_server(size, num, port);
    return ret;
}
