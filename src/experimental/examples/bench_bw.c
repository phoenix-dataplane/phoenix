#include "bench_send_bw.h"
#include "bench_read_bw.h"
#include "bench_write_bw.h"

int main(int argc, char **argv)
{
    Context ctx;
    ctx.opt = SEND;
    ctx.num = 1000, ctx.size = (1 << 16), ctx.client = false;
    ctx.ip = "127.0.0.1", ctx.port = "5000";

    parse(&ctx, argc, argv);

    if (ctx.client)
        printf("client\n");
    else
        printf("server\n");

    printf("num: %d, size: %d\n", ctx.num, ctx.size);

    int ret = 0;
    switch (ctx.opt)
    {
    case SEND:
        printf("send data from client to server\n");
        if (ctx.client)
            ret = run_send_bw_client(&ctx);
        else
            ret = run_send_bw_server(&ctx);
        break;
    case WRITE:
        printf("write data from client to server\n");
        if (ctx.client)
            ret = run_write_bw_client(&ctx);
        else
            ret = run_write_bw_server(&ctx);
        break;
    case READ:
        printf("read data from server to client\n");
        if (ctx.client)
            ret = run_read_bw_client(&ctx);
        else
            ret = run_read_bw_server(&ctx);
    }
    return 0;
}
