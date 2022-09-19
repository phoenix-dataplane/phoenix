#!/usr/bin/env python3
from typing import List
import glob
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]

# output in csv: Model,Solution,Avg Completion Time (us)

def convert_to_micros(dura: str) -> float:
    if dura.endswith('ns'):
        return 1e-3 * float(dura[:-2])
    if dura.endswith('µs'):
        return float(dura[:-3]) # unicode
    if dura.endswith('ms'):
        return 1e3 * float(dura[:-2])
    if dura.endswith('s'):
        return 1e6 * float(dura[:-2])
    raise Exception(f"unknown {dura}")


def convert_model_name(name: str) -> float:
    if name == 'inception_v3':
        return 'InceptionV3'
    if name == 'mobile_net':
        return 'MobileNet'
    if name == 'efficient_net':
        return 'EfficientNet'
    raise Exception(f"unknown {name}")

def get_completion_time(path: str) -> List[float]:
    jcts = []
    with open(path, 'r') as fin:
        for line in fin:
            words = line.strip().split(' ')
            if words[2].startswith('duration') and words[-1] == 'Mrps':
                dura = words[3].strip(',')
                dura = convert_to_micros(dura)
                jcts.append(dura)
    drop_rate = 0.15
    to_drop = int(len(jcts) * drop_rate)
    # print(to_drop)
    return jcts[to_drop:]


def load_result(solution, f: str):
    # print(f)
    model = f.split('/')[-2]
    model = convert_model_name(model)
    jcts = get_completion_time(f)
    for t in jcts:
        print(f'{model},{solution},{t}')

print('Model,Solution,Avg Completion Time (us)')

solution = 'w/o Scheduler'
# '/tmp/mrpc-eval/benchmark/parameter_server_without_scheduler/inception_v3/rpc_bench_plus_client_danyang-05.stdout'
for f in glob.glob(OD+"/benchmark/parameter_server_without_scheduler/*/rpc_bench_plus_client_danyang-05.stdout"):
    load_result(solution, f)

solution = 'w/ Scheduler'
for f in glob.glob(OD+"/benchmark/parameter_server_with_scheduler/*/rpc_bench_plus_client_danyang-05.stdout"):
    load_result(solution, f)
