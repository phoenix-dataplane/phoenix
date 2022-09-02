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
os.environ['KOALA_PREFIX'] = config['env']['KOALA_PREFIX']

os.chdir(workdir)
os.makedirs(OD+"/policy/ratelimit", exist_ok=True)
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

with open(OD+"/policy/list.json") as f:
    data = json.load(f)
addon_eid = None
for subscription in data:
    pid = subscription["pid"]
    sid = subscription["sid"]
    engines = [x[1] for x in subscription["engines"]]
    for (eid, engine) in subscription["engines"]:
        if engine == "RateLimitEngine":
            addon_eid = eid

rates = [
    500000,
    900000,
]
for rate in rates:
    subprocess.run([
        "cargo",
        "run",
        "--release",
        "--bin",
        "ratelimitctl",
        "--",
        "--eid",
        str(addon_eid),
        "-r",
        str(rate),
        "-b",
        str(rate)
    ])
    time.sleep(1)

detach_config = os.path.join(SCRIPTDIR, "detach.toml")
subprocess.run([
    "cargo",
    "run",
    "--release",
    "--bin",
    "addonctl",
    "--",
    "--config",
    detach_config,
    "--pid",
    str(mrpc_pid),
    "--sid",
    str(mrpc_sid),
])

workload.wait()
