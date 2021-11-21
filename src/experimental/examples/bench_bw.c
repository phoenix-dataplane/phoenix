#include <getopt.h>
#include <stdbool.h>
#include <string.h>
#include "bench_send_bw.h"

int main(int argc, char **argv)
{
    Context ctx;
    ctx.opt = SEND;
    ctx.num = 1000, ctx.size = (1 << 16), ctx.client = false;
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
        else if (strcmp(argv[optind], "read") == 0)
            ctx.opt = READ;
    }

    printf("num: %d, size: %d", ctx.num, ctx.size);

    int ret = 0;
    switch (ctx.opt)
    {
    case SEND:
        printf("send perf\n");
        if (ctx.client)
            ret = run_send_bw_client(&ctx);
        else
            ret = run_send_bw_server(&ctx);
        break;
        // case WRITE:
        //     printf("write perf\n");
        //     if (ctx.client)
        //         ret = run_write_bw_client(&ctx);
        //     else
        //         ret = run_write_bw_server(&ctx);
        //     break;
        // case READ:
        //     printf("read perf\n");
        //     if (ctx.client)
        //         ret = run_read_bw_client(&ctx);
        //     else
        //         ret = run_read_bw_server(&ctx);
    }
    return ret;
}
