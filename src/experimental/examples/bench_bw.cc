#include "bench_read_bw.h"
#include "bench_send_bw.h"
#include "bench_write_bw.h"

int main(int argc, char **argv)
{
    Context ctx;
    ctx.tst = BW;
    ctx.verb = SEND;
    ctx.num = 0, ctx.size = (1 << 16), ctx.client = false;

    parse(&ctx, argc, argv);
    int ret = set_params(&ctx);
    if (ret)
        goto out;
    if (ctx.num == 0)
        ctx.num = (ctx.verb == WRITE) ? 5000 : 1000;

    printf("num: %u, size: %lu\n", ctx.num, ctx.size);

    switch (ctx.verb)
    {
    case SEND:
        printf("Send data from client to server\n");
        if (ctx.client)
            ret = run_send_bw_client(&ctx);
        else
            ret = run_send_bw_server(&ctx);
        break;
    case WRITE:
        printf("Write data from client to server\n");
        if (ctx.client)
            ret = run_write_bw_client(&ctx);
        else
            ret = run_write_bw_server(&ctx);
        break;
    case READ:
        printf("Read data from server to client\n");
        if (ctx.client)
            ret = run_read_bw_client(&ctx);
        else
            ret = run_read_bw_server(&ctx);
    }

    free_ctx(&ctx);
out:
    if (ret)
        printf("error %d\n", ret);
    return 0;
}
