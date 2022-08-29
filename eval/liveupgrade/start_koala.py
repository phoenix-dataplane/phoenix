import subprocess
import pathlib
import os
import toml
SCRIPTDIR = pathlib.Path(__file__).parent.resolve()
CONFIG_PATH = os.path.join(SCRIPTDIR, "config.toml")

config = toml.load(CONFIG_PATH)
workdir = config["workdir"]
workdir = os.path.expanduser(workdir)

os.chdir(workdir)
os.makedirs("/tmp/mrpc-eval/", exist_ok=True)
try:
    koala = subprocess.Popen([
        "cargo", 
        "run",
        "--release",
        "--bin",
        "launcher",
        "--",
        "-o",
        "/tmp/mrpc-eval",
        "--benchmark", 
        os.path.join(SCRIPTDIR, "launch_koala.toml"), 
        "--configfile",
        os.path.join(SCRIPTDIR, "config.toml"),
        "--timeout",
        "600",
    ])
except KeyboardInterrupt:
    os.kill(koala.pid, signal.SIGTERM)