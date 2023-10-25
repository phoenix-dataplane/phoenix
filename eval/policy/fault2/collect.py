#!/usr/bin/env python3
from typing import List
import glob
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]


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


def get_rate(path: str) -> List[float]:
    rates = []
    with open(path, 'r') as fin:
        for line in fin:
            words = line.strip().split(' ')
            if words[-3] == 'rps,':
                rate = float(words[-4])
                rates.append(rate)
    return rates[1:]


def load_result(sol_before, sol_after, f: str):
    # print(f)
    rates = get_rate(f)
    before = rates[5:25]
    after = rates[-25:-5]
    for r in before:
        print(f'{round(r/1000,2)},{sol_before},w/o Fault')
    for r in after:
        print(f'{round(r/1000,2)},{sol_after},w/ Fault')


for f in glob.glob(OD+"/policy/fault2/rpc_bench_tput_32b/rpc_bench_client_danyang-04.stdout"):
    load_result('mRPC', 'Native mRPC', f)
