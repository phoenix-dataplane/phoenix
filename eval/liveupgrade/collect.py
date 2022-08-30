import pandas as pd
import datetime

TIME_FMT = "%Y-%m-%dT%H:%M:%S.%fZ"
TIME_FMT_BACKEND = "%Y-%m-%d %H:%M:%S.%f"

def read_rates(path): 
    timestamps = []
    rates = []
    with open(path, 'rt') as f:
        for line in f.readlines():
            columns = line.strip().split(', ')
            if columns[-1].endswith("Gb/s"):
                date_time = datetime.datetime.strptime(columns[0][:27], TIME_FMT)
                ts = round(date_time.timestamp() * 1000)
                rate = float(columns[1][:-4])
                timestamps.append(ts)
                rates.append(rate)
    
    # remove first and last 10 measurement
    timestamps = timestamps[10:-10]
    rates = rates[10:-10]
    data = pd.DataFrame(
        data = {
            "timestamp": timestamps,
            "rate": rates,
        }
    )
    return data


def read_backend_log(client_log, server_log):
    operations = []
    timestamps = []
    with open(server_log, 'rt') as f: 
        for line in f.readlines():
            lb = line.find('[')
            rb = line.find(']')
            meta = line[lb+1:rb]
            log = line[rb + 2:]
            date_time = datetime.datetime.strptime(meta[:26], TIME_FMT_BACKEND)
            ts = round(date_time.timestamp() * 1000)
            if "Receive backend upgrade request from koalactl" in log:
                operations.append("upgrade_server")
                timestamps.append(ts)
    with open(client_log, 'rt') as f: 
        for line in f.readlines():
            lb = line.find('[')
            rb = line.find(']')
            meta = line[lb+1:rb]
            log = line[rb + 2:]
            date_time = datetime.datetime.strptime(meta[:26], TIME_FMT_BACKEND)
            ts = round(date_time.timestamp() * 1000)
            if "Receive backend upgrade request from koalactl" in log:
                operations.append("upgrade_client")
                timestamps.append(ts)

    logs = pd.DataFrame (
        data = {
            "timestamp": timestamps,
            "operation": operations,
        }
    )
    return logs

rates = read_rates("/tmp/mrpc-eval/liveupgrade/rpc_bench_tput_32b/rpc_bench_client_danyang-05.stdout")
SERVER = "/tmp/mrpc-eval/launch_koala/koala_danyang-06.stdout"
CLIENT = "/tmp/mrpc-eval/launch_koala/koala_danyang-05.stdout"
logs = read_backend_log(CLIENT, SERVER)
rates.to_csv(
    "/tmp/mrpc-eval/liveupgrade/rpc_bench_tput_32b/rates.csv",
    sep=",",
    header=True,
)
logs.to_csv(
    "/tmp/mrpc-eval/liveupgrade/rpc_bench_tput_32b/server_log.csv",
    sep=",",
    header=True,
)

all_ts = [x for x in rates["timestamp"]]
all_ts.extend([x for x in logs['timestamp']])
base_ts = min(all_ts)
rates["timestamp"] -= base_ts
logs["timestamp"] -= base_ts
rates["rate"] /= 1000

rates["timestamp"] /= 1000
logs["timestamp"] /= 1000
all_ts = [x for x in rates["timestamp"]]
all_ts.extend([x for x in logs['timestamp']])
max_ts = max(all_ts)

import matplotlib.pyplot as plt
def find_closest_rate(rates, ts):
    min_diff = None
    target_rate = None
    for _, row in rates.iterrows():
        if min_diff is None or (row["timestamp"] > ts and row["timestamp"] - ts < min_diff):
            min_diff = row["timestamp"] - ts
            target_rate = row["rate"]
    return target_rate

fig, ax = plt.subplots(figsize=(8, 6))
ax.fill_between(rates["timestamp"], rates["rate"], step="pre", color="#a29bfe", alpha=0.5, linewidth=0)
ax.set_xlim(0, max_ts)
ax.set_ylim(0, 1000)
ax.set_xticks([0, 5, 10, 15])
ax.set_xlabel("Time (sec)", fontsize=30)
ax.set_ylabel("Rate (Krps)", fontsize=30)
ax.tick_params(axis="x", labelsize=25)
ax.tick_params(axis="y", labelsize=25)
for _, row in logs.iterrows():
    ts = row["timestamp"]
    op = row["operation"]
    rate = None
    if op == "upgrade_client":
        anno = "U/C"
    elif op == "upgrade_server":
        anno = "U/S"
    y_coord = find_closest_rate(rates, ts)
    ax.annotate(
        text=anno,
        xy=(ts, y_coord),
        xytext=(0, 25),
        textcoords="offset pixels",
        horizontalalignment="center",
        arrowprops={
            "arrowstyle": "->",
        },
        fontsize=20,
    )

plt.savefig("/tmp/mrpc-eval/liveupgrade/rpc_bench_tput_32b/rate.pdf", bbox_inches='tight')