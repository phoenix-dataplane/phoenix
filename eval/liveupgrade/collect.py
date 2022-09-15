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
    "/tmp/mrpc-eval/liveupgrade/rates_32b.csv",
    sep=",",
    header=True,
)
logs.to_csv(
    "/tmp/mrpc-eval/liveupgrade/server_log.csv",
    sep=",",
    header=True,
)
rates = read_rates("/tmp/mrpc-eval/liveupgrade/rpc_bench_tput_2kb/rpc_bench_client_danyang-04.stdout")
rates.to_csv(
    "/tmp/mrpc-eval/liveupgrade/rates_2kb.csv",
    sep=",",
    header=True,
)