#!/usr/bin/env python3
from typing import List
import glob
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]

# x-axis: message size in KB
# y-axis: goodput in Gb/s
# output in csv: solution,RPC size,goodput


def convert_msg_size(s: str) -> int:
    if s.endswith('gb'):
        return int(s[:-2]) * 1024 * 1024 * 1024
    if s.endswith('mb'):
        return int(s[:-2]) * 1024 * 1024
    if s.endswith('kb'):
        return int(s[:-2]) * 1024
    if s.endswith('b'):
        return int(s[:-1])

    raise ValueError(f"unknown input: {s}")


def get_goodput(path: str) -> List[float]:
    goodputs = []
    with open(path, 'r') as fin:
        for line in fin:
            words = line.strip().split(' ')
            if words[-1] == 'Gb/s':
                tput = words[-2]
                goodputs.append(tput)
    return goodputs[1:]


# rpc_bench_tput_128kb/rpc_bench_client_danyang-05.stdout
# /tmp/mrpc-eval/benchmark/rpc_bench_tput_32/rpc_bench_tput_512b/rpc_bench_client_danyang-05.stderr

xticks = [(2 << i) for i in range(0, 14, 2)]


def load_result(solution, f: str):
    # print(f)
    msg_size_text = f.split('/')[-2].split('_')[-1]
    msg_size = convert_msg_size(msg_size_text)
    if msg_size < 2048:
        return
    msg_size_kb = msg_size // 1024
    if msg_size_kb not in xticks:
        return
    goodputs = get_goodput(f)
    for g in goodputs:
        print(f'{msg_size_kb},{g},{solution}')


solution = 'mRPC (32)'
for f in glob.glob(OD+"/benchmark/rpc_bench_tput_rdma_32/rpc_bench_tput_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)

solution = 'mRPC (1)'
for f in glob.glob(OD+"/benchmark/rpc_bench_tput_rdma_1/rpc_bench_tput_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)

solution = 'mRPC-TCP (32)'
for f in glob.glob(OD+"/benchmark/rpc_bench_tput_tcp_32/rpc_bench_tput_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)

solution = 'mRPC-TCP (1)'
for f in glob.glob(OD+"/benchmark/rpc_bench_tput_tcp_1/rpc_bench_tput_*/rpc_bench_client_danyang-05.stdout"):
    load_result(solution, f)
