#!/usr/bin/env python3
import json
import subprocess
import pathlib
import os
import time
import toml
import datetime
import sys

OD = "/tmp/mrpc-eval"
if len(sys.argv) >= 2:
    OD = sys.argv[1]

TIME_FMT = "%Y-%m-%dT%H:%M:%S.%fZ"

SCRIPTDIR = pathlib.Path(__file__).parent.resolve()
CONFIG_PATH = os.path.join(SCRIPTDIR, "config.toml")

config = toml.load(CONFIG_PATH)
workdir = config["workdir"]
workdir = os.path.expanduser(workdir)
os.environ['PHOENIX_PREFIX'] = config['env']['PHOENIX_PREFIX']

os.chdir(workdir)
os.makedirs(OD+"/qos", exist_ok=True)
workload_lat = subprocess.Popen([
    "cargo",
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    OD,
    "--benchmark",
    os.path.join(SCRIPTDIR, "brusty_latency.toml"),
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
    OD+"/policy/qos/list.json"
])
time.sleep(1)
with open(OD+"/policy/qos/list.json") as f:
    data = json.load(f)
mrpc_pid_lat = None
mrpc_sid_lat = None
for subscription in data:
    pid = subscription["pid"]
    sid = subscription["sid"]
    engines = [x[1] for x in subscription["engines"]]
    if "MrpcEngine" in engines:
        mrpc_pid_lat = pid
        mrpc_sid_lat = sid
attach_config = os.path.join(SCRIPTDIR, "attach_high_priority.toml")
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
    str(mrpc_pid_lat),
    "--sid",
    str(mrpc_sid_lat),
])


workload_bd = subprocess.Popen([
    "cargo",
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    OD,
    "--benchmark",
    os.path.join(SCRIPTDIR, "brusty_bandwidth.toml"),
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
    OD+"/policy/qos/list.json"
])
time.sleep(1)
with open(OD+"/policy/qos/list.json") as f:
    data = json.load(f)
mrpc_pid_bd = None
mrpc_sid_bd = None
for subscription in data:
    pid = subscription["pid"]
    sid = subscription["sid"]
    engines = [x[1] for x in subscription["engines"]]
    if "MrpcEngine" in engines and pid != mrpc_pid_lat:
        mrpc_pid_bd = pid
        mrpc_sid_bd = sid
attach_config = os.path.join(SCRIPTDIR, "attach_low_priority.toml")
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
    str(mrpc_pid_bd),
    "--sid",
    str(mrpc_sid_bd),
])

workload_lat.wait()
workload_bd.wait()

results = ""
with open(OD+"/policy/qos/latency_app/rpc_bench_brusty_client_danyang-05.stdout") as f:
    line = f.readlines()[-1]
    line = line.strip().split(", ")
    results += "With QoS Policy:\n"
    results += "Latency-sensitive workload: " + ", ".join(line[1:]) + "\n"
with open(OD+"/policy/qos/bandwidth_app/rpc_bench_brusty_client_danyang-05.stdout") as f:
    line = f.readlines()[-2]
    line = line.strip().split(", ")
    results += "Bandwidth-sensitive workload: " + ", ".join(line[1:]) + "\n"
    results += '\n'

workload_lat = subprocess.Popen([
    "cargo",
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    OD,
    "--benchmark",
    os.path.join(SCRIPTDIR, "brusty_latency.toml"),
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"),
], stdout=subprocess.DEVNULL)
time.sleep(5)
workload_bd = subprocess.Popen([
    "cargo",
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    OD,
    "--benchmark",
    os.path.join(SCRIPTDIR, "brusty_bandwidth.toml"),
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"),
], stdout=subprocess.DEVNULL)
workload_lat.wait()
workload_bd.wait()

with open(OD+"/policy/qos/latency_app/rpc_bench_brusty_client_danyang-05.stdout") as f:
    line = f.readlines()[-1]
    line = line.strip().split(", ")
    results += "Without QoS Policy:\n"
    results += "Latency-sensitive workload: " + ", ".join(line[1:]) + "\n"
with open(OD+"/policy/qos/bandwidth_app/rpc_bench_brusty_client_danyang-05.stdout") as f:
    line = f.readlines()[-2]
    line = line.strip().split(", ")
    results += "Bandwidth-sensitive workload: " + ", ".join(line[1:]) + "\n"

with open(OD+"/policy/qos/result.txt", 'wt') as f:
    print(results, file=f)

print("===== Results =====")
print(results)