#!/usr/bin/env python3
from typing import List
import glob
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]


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
    rates = get_rate(f)
    before = rates[5:25]
    after = rates[-25:-5]
    for r in before:
        print(f'{round(r/1000,2)},{sol_before},w/o Limit')
    for r in after:
        print(f'{round(r/1000,2)},{sol_after},w/ Limit')


for f in glob.glob(OD+"/policy/ratelimit/rpc_bench_tput_32b/rpc_bench_client_danyang-05.stdout"):
    load_result('mRPC', 'mRPC', f)
