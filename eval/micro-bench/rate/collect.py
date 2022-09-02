#!/usr/bin/env python3
from typing import List
import glob
import numpy as np
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]

# x-axis: # client threads
# y-axis: goodput in Mrpcs
# output in csv: Client threads,RPC Rate (Mrps),Solution


def get_goodput(path: str) -> List[float]:
    goodputs = []
    with open(path, 'r') as fin:
        for line in fin:
            words = line.strip().split(' ')
            if words[-1] == 'Gb/s':
                rate = words[-2]
                goodputs.append(rate)
    return goodputs[1:]


def get_rate(ncores: int, path: str) -> List[float]:
    rates = [[] for i in range(ncores)]
    with open(path, 'r') as fin:
        for line in fin:
            words = line.strip().split(' ')
            if words[0] == 'Thread':
                tid = int(words[1][:-1])
            if words[3] == 'rps,':
                rate = words[2]
                rates[tid].append(rate)
    min_len = 99999
    for i, rate in enumerate(rates):
        assert len(rate) > 2, f'{rates}'
        rates[i] = rate[2:]
        min_len = min(min_len, len(rates[i]))
    agg_rates = []
    for i in range(min_len):
        s = 0
        for tid in range(ncores):
            s += float(rates[tid][i])
        agg_rates.append(s)
    return agg_rates

# /tmp/mrpc-eval/benchmark/rpc_bench_rate_32/rpc_bench_rate_32b_1c/rpc_bench_client_danyang-05.stderr


xticks = [(1 << i) for i in range(0, 4)]


def load_result(solution, f: str):
    # print(f)
    num_cores = f.split('/')[-2].split('_')[-1]
    assert num_cores.endswith('c'), f'{num_cores}'
    num_cores = int(num_cores[:-1])
    rates = get_rate(num_cores, f)
    for r in rates:
        print(f'{num_cores},{r / 1e6},{solution}')


solution = 'mRPC'
for f in glob.glob(OD+"/benchmark/rpc_bench_rate_rdma_32/rpc_bench_rate_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)

solution = 'mRPC-TCP'
for f in glob.glob(OD+"/benchmark/rpc_bench_rate_tcp_32/rpc_bench_rate_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)
