#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <string.h>
#include <getopt.h>
#include <time.h>
#include <sys/time.h>
#include <unistd.h>
#include <errno.h>
#include <stdint.h>
#include "get_clock.h"

uint64_t Now64()
{
    struct timespec tv;
    clock_gettime(CLOCK_REALTIME, &tv);
    return (uint64_t)tv.tv_sec * 1000000llu + (uint64_t)tv.tv_nsec;
}

int main()
{
    int n = 10000000;
    uint64_t t;
    uint64_t start = Now64();
    for (int i = 0; i < n; i++)
        t = get_cycles();
    uint64_t finish = Now64();
    uint64_t sum = finish - start;
    printf("%lu, %.2lf\n", sum, 1.0 * sum / n);
    return 0;
}
