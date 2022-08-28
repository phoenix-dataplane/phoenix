import pandas as pd
import datetime

TIME_FMT = "%Y-%m-%dT%H:%M:%S.%fZ"
TIME_FMT_BACKEND = "%Y-%m-%d %H:%M:%S.%f"
RATES = [
    100000,
    500000,
    900000,
]
RATES = [x // 1000 for x in RATES]

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


def read_backend_log(path):
    operations = []
    timestamps = []
    with open(path, 'rt') as f: 
        for line in f.readlines():
            lb = line.find('[')
            rb = line.find(']')
            meta = line[lb+1:rb]
            log = line[rb + 2:]
            date_time = datetime.datetime.strptime(meta[:26], TIME_FMT_BACKEND)
            ts = round(date_time.timestamp() * 1000)
            if "Receive attach addon request from koalactl" in log:
                operations.append("attach")
                timestamps.append(ts)
            elif "Receive engine request" in log:
                operations.append("request")
                timestamps.append(ts)
            elif "Receive detach addon request from koalactl" in log:
                operations.append("detach")
                timestamps.append(ts)
    logs = pd.DataFrame (
        data = {
            "timestamp": timestamps,
            "operation": operations,
        }
    )
    return logs

rates = read_rates("/tmp/mrpc-eval/policy/ratelimit/rpc_bench_tput_32b/rpc_bench_client_danyang-02.stdout")
logs = read_backend_log("/tmp/mrpc-eval/launch_koala/koala_danyang-02.stdout")
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

fig, ax = plt.subplots(figsize=(8, 6))
ax.fill_between(rates["timestamp"], rates["rate"], step="pre", color="#a29bfe", alpha=0.7, linewidth=0)
ax.set_xlim(0, max_ts)
ax.set_ylim(0, 1000)
ax.set_xticks([0, 15, 30, 45])
ax.set_xlabel("Time", fontsize=30)
ax.set_ylabel("Rate (Krps)", fontsize=30)
ax.tick_params(axis="x", labelsize=25)
ax.tick_params(axis="y", labelsize=25)
for _, row in logs.iterrows():
    ts = row["timestamp"]
    op = row["operation"]
    rate = None
    anno = None
    if op == "attach":
        rate = RATES.pop(0)
        anno = "{:d} Krps".format(rate)
    elif op == "request":
        rate = RATES.pop(0)
        if len(rates) == 0:
            anno = "Unlimited"
            rate = 860
        else:
            anno = "{:d} Krps".format(rate)
    elif op == "detach":
        anno = "Remove RL"
    if rate is None:
        y_coord = rates["rate"][0]
    else:
        y_coord = rate
    ax.annotate(
        text=anno,
        xy=(ts, y_coord),
        xytext=(0, 25),
        textcoords="offset pixels",
        horizontalalignment="center",
        arrowprops={
            "arrowstyle": "->",
        },
        fontsize=16,
    )

plt.savefig("/tmp/mrpc-eval/policy/ratelimit/rate.pdf", bbox_inches='tight')