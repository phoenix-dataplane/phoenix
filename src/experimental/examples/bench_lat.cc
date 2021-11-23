#include "bench_read_lat.h"
#include "bench_send_lat.h"
#include "bench_write_lat.h"
#include <getopt.h>
#include <stdbool.h>
#include <string.h>

int main(int argc, char **argv)
{
    Context ctx;
    ctx.opt = SEND;
    ctx.num = 1000, ctx.warmup = 100, ctx.size = 4, ctx.client = false;

    parse(&ctx, argc, argv);
    int ret = set_params(&ctx);
    if (ret)
        goto out;

    ctx.size = max(ctx.size, (size_t)4);
    if (ctx.num < ctx.warmup)
        ctx.num += ctx.warmup;
    printf("num: %u, size: %lu, warmup: %u\n", ctx.num, ctx.size, ctx.warmup);

    switch (ctx.opt)
    {
    case SEND:
        printf("Send data from client to server\n");
        if (ctx.client)
            ret = run_send_lat_client(&ctx);
        else
            ret = run_send_lat_server(&ctx);
        break;
    case WRITE:
        printf("Write data from client to server\n");
        if (ctx.client)
            ret = run_write_lat_client(&ctx);
        else
            ret = run_write_lat_server(&ctx);
        break;
    case READ:
        printf("Read data from server to client\n");
        if (ctx.client)
            ret = run_read_lat_client(&ctx);
        else
            ret = run_read_lat_server(&ctx);
    }

out:
    if (ret)
        printf("error %d\n", ret);
    return 0;
}
