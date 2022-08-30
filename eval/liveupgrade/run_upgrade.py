import json
import subprocess
import pathlib
import os
import time
import toml
SCRIPTDIR = pathlib.Path(__file__).parent.resolve()
CONFIG_PATH = os.path.join(SCRIPTDIR, "config.toml")

config = toml.load(CONFIG_PATH)
workdir = config["workdir"]
workdir = os.path.expanduser(workdir)

os.chdir(workdir)
os.makedirs("/tmp/mrpc-eval/liveupgrade/", exist_ok=True)
workload = subprocess.Popen([
    "cargo", 
    "run",
    "--release",
    "--bin",
    "launcher",
    "--",
    "-o",
    "/tmp/mrpc-eval",
    "--benchmark", 
    os.path.join(SCRIPTDIR, "rpc_bench_tput_32b.toml"), 
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"), 
], stdout=subprocess.DEVNULL)
time.sleep(10)
subprocess.run([
    "cargo", 
    "run", 
    "--release",
    "--bin",
    "launcher", 
    "--",
    "--benchmark",
    os.path.join(SCRIPTDIR, "upgrade_server.toml"),
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"), 
])
time.sleep(5)
subprocess.run([
    "cargo", 
    "run", 
    "--release",
    "--bin",
    "launcher", 
    "--",
    "--benchmark",
    os.path.join(SCRIPTDIR, "upgrade_client.toml"),
    "--configfile",
    os.path.join(SCRIPTDIR, "config.toml"), 
])
workload.wait()