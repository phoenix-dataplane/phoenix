#include <getopt.h>
#include <stdbool.h>
#include <string.h>
#include "bench_send_lat.h"
#include "bench_write_lat.h"

int main(int argc, char **argv)
{
    Context ctx;
    ctx.opt = SEND;
    ctx.num = 1000, ctx.warmup = 100, ctx.size = 4, ctx.client = false;
    ctx.ip = "127.0.0.1", ctx.port = "5000";
    int op;

    while ((op = getopt(argc, argv, "c:p:n:s:")) != -1)
    {
        switch (op)
        {
        case 'c':
            ctx.client = true;
            ctx.ip = optarg;
            break;
        case 'p':
            ctx.port = optarg;
            break;
        case 'n':
            ctx.num = atoi(optarg);
            break;
        case 's':
            ctx.size = atoi(optarg);
            break;
        }
    }
    if (optind < argc)
    {
        if (strcmp(argv[optind], "write") == 0)
            ctx.opt = WRITE;
    }

    ctx.size = MAX(ctx.size, 4);
    if (ctx.num < ctx.warmup)
        ctx.num += ctx.warmup;
    printf("num: %d, size: %d, warmup: %d\n", ctx.num, ctx.size, ctx.warmup);

    int ret = 0;
    switch (ctx.opt)
    {
    case SEND:
        printf("send perf\n");
        if (ctx.client)
            ret = run_send_lat_client(&ctx);
        else
            ret = run_send_lat_server(&ctx);
        break;
    case WRITE:
        printf("write perf\n");
        if (ctx.client)
            ret = run_write_lat_client(&ctx);
        else
            ret = run_write_lat_server(&ctx);
        break;
    }
    return ret;
}
