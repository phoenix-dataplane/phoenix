#!/usr/bin/env python3
import json
import subprocess
import pathlib
import os
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
workdir = os.path.expanduser(workdir)
os.environ['PHOENIX_PREFIX'] = config['env']['PHOENIX_PREFIX']

os.chdir(workdir)
os.makedirs(OD+"/policy/null", exist_ok=True)
workload = subprocess.Popen([
    "cargo",
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    OD,
    "--benchmark",
    os.path.join(SCRIPTDIR, "rpc_bench_tput_32b.toml"),
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"),
], stdout=subprocess.DEVNULL)
time.sleep(5)

subprocess.run([
    "cargo",
    "run",
    "--release",
    "--bin",
    "list",
    "--",
    "--dump",
    OD+"/policy/list.json"
])
with open(OD+"/policy/list.json") as f:
    data = json.load(f)
mrpc_pid = None
mrpc_sid = None
for subscription in data:
    pid = subscription["pid"]
    sid = subscription["sid"]
    engines = [x[1] for x in subscription["engines"]]
    if "MrpcEngine" in engines:
        mrpc_pid = pid
        mrpc_sid = sid

attach_config = os.path.join(SCRIPTDIR, "attach.toml")
subprocess.run([
    "cargo",
    "run",
    "--release",
    "--bin",
    "addonctl",
    "--",
    "--config",
    attach_config,
    "--pid",
    str(mrpc_pid),
    "--sid",
    str(mrpc_sid),
])
time.sleep(1)
subprocess.run([
    "cargo",
    "run",
    "--release",
    "--bin",
    "list",
    "--",
    "--dump",
    OD+"/policy/list.json"
])
time.sleep(1)


workload.wait()
