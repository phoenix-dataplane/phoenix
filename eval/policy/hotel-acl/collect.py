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
            if words[1] == 'rps,':
                rate = words[0]
                rates.append(rate)
    assert len(rates) >= 1 + 10 + 15
    return rates[1:]


xticks = [(2 << i) for i in range(0, 14, 2)]


def load_result(sol_before, sol_after, f: str):
    # print(f)
    rates = get_rate(f)
    before = rates[:10]
    after = rates[-15:]
    for r in before:
        print(f'{r},{sol_before}')
    for r in after:
        print(f'{r},{sol_after}')


for f in glob.glob(OD+"/benchmark/hotel_reservation/hotel_reservation_client_danyang-05.stdout"):
    load_result('mRPC (w/o ACL)', 'mRPC (w/ ACL)', f)
