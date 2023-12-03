#!/usr/bin/env python3
import json
import subprocess
import pathlib
import os
from os.path import dirname
import time
import toml
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]

SCRIPTDIR = pathlib.Path(__file__).parent.resolve()
CONFIG_PATH = os.path.join(SCRIPTDIR, "config.toml")

config = toml.load(CONFIG_PATH)
workdir = config["workdir"]
workdir = dirname(dirname(os.path.expanduser(workdir)))
env = {**os.environ, **config['env']}

os.chdir(workdir)
os.makedirs(f"{OD}/policy/null", exist_ok=True)

cmd = f'''cargo run --release -p benchmark --bin launcher -- -o {OD} --timeout=120
--benchmark {os.path.join(SCRIPTDIR, 'rpc_bench_tput_32b.toml')} 
--configfile { os.path.join(SCRIPTDIR, 'config.toml')}'''
workload = subprocess.Popen(cmd.split())
time.sleep(30)

list_cmd = f"cargo run --release --bin list -- --dump {OD}/policy/list.json"
subprocess.run(list_cmd.split(), env=env)

with open(f"{OD}/policy/list.json", "r") as fin:
    content = fin.read()
    print(content)
    data = json.loads(content)
mrpc_pid = None
mrpc_sid = None
for subscription in data:
    pid = subscription["pid"]
    sid = subscription["sid"]
    engines = [x[1] for x in subscription["engines"]]
    if "MrpcEngine" in engines:
        mrpc_pid = pid
        mrpc_sid = sid

print("Start to attach policy")
attach_config = os.path.join(SCRIPTDIR, "attach.toml")
attach_cmd = f"cargo run --release --bin addonctl -- --config {attach_config} --pid {mrpc_pid} --sid {mrpc_sid}"
subprocess.run(attach_cmd.split(), env=env)

subprocess.run(list_cmd.split(), env=env)
with open(f"{OD}/policy/list.json", "r") as fin:
    print(fin.read())

workload.wait()
