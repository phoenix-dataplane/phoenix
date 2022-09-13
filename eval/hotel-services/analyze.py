import pandas as pd
import csv
import os
import numpy as np

DATA_DIR = os.path.expanduser("~/nfs/mrpc-microservices-eval")

columns = [
    "type",
    "service",
    "index",
    "duration",
]
dtypes = {
    "type": str,
    "service": str,
    "index": int,
    "duration": int,
}

geo = pd.read_csv(
    os.path.join(DATA_DIR, "geo.csv"),
    sep=",",
    header=None,
    names=columns,
    dtype=dtypes,
)
rate = pd.read_csv(
    os.path.join(DATA_DIR, "rate.csv"),
    sep=",",
    header=None,
    names=columns,
    dtype=dtypes,
)
profile = pd.read_csv(
    os.path.join(DATA_DIR, "profile.csv"),
    sep=",",
    header=None,
    names=columns,
    dtype=dtypes,
)
search = pd.read_csv(
    os.path.join(DATA_DIR, "search.csv"),
    sep=",",
    header=None,
    names=columns,
    dtype=dtypes,
)
frontend = pd.read_csv(
    os.path.join(DATA_DIR, "frontend.csv"),
    sep=",",
    header=None,
    names=columns,
    dtype=dtypes,
)
logs = pd.concat([geo, rate, profile, search, frontend], ignore_index=True)

request_latency_table = dict()
for _, row in logs.iterrows():
    index = row["index"]
    ty = row["type"]
    service = row["service"]
    dura = row["duration"]
    if index not in request_latency_table:
        request_latency_table[index] = dict()
    if service not in request_latency_table[index]:
        request_latency_table[index][service] = dict()

    if ty == "Proc":
        request_latency_table[index][service]["app_proc"] = dura
    elif ty == "EndToEnd":
        request_latency_table[index][service]["end_to_end"] = dura
    else:
        raise ValueError("invalid record type")

for index, service_lats in request_latency_table.items():
    for service, lat in service_lats.items():
        if service in ["geo", "rate", "profile"]:
            request_latency_table[index][service]["network"] = lat["end_to_end"] - lat["app_proc"]
        elif service == "search": 
            lat["app_proc"] = service_lats["geo"]["app_proc"] + service_lats["rate"]["app_proc"] 
            request_latency_table[index][service]["network"] = lat["end_to_end"] - lat["app_proc"]
        else:
            raise ValueError("invalid service")
    service_lats["frontend"] = {
        "app_proc": service_lats["search"]["app_proc"] + service_lats["profile"]["app_proc"],
        "network": service_lats["search"]["network"] + service_lats["profile"]["network"],
        "end_to_end": service_lats["search"]["end_to_end"] + service_lats["profile"]["end_to_end"],
    }
    assert(np.isclose(
        service_lats["frontend"]["end_to_end"], 
        service_lats["frontend"]["app_proc"] + service_lats["frontend"]["network"]
    ))

services_traces = dict()
for _, service_lats in request_latency_table.items():
    for service, lat in service_lats.items():
        if service not in services_traces:
            services_traces[service] = {
                "app_proc": [],
                "network": [],
                "end_to_end": [],
            }
        services_traces[service]["app_proc"].append(lat["app_proc"])
        services_traces[service]["network"].append(lat["network"])
        services_traces[service]["end_to_end"].append(lat["end_to_end"])
    
p95_latency = dict()
p99_latency = dict()
for service, traces in services_traces.items():
    traces["app_proc"] = np.array(traces["app_proc"], dtype=np.int64)
    traces["network"] = np.array(traces["network"], dtype=np.int64)
    traces["end_to_end"] = np.array(traces["end_to_end"], dtype=np.int64)

    indices = np.argsort(traces["end_to_end"])
    traces["end_to_end"] = traces["end_to_end"][indices]
    traces["app_proc"] = traces["app_proc"][indices]
    traces["network"] = traces["network"][indices]
    
    p95_latency[service] = {
        "AppProc": np.percentile(traces["app_proc"], q=95) / 1000,
        "Network": np.percentile(traces["network"], q=95) / 1000,
        "EndToEnd": np.percentile(traces["end_to_end"], q=95) / 1000,
    }
    p99_latency[service] = {
        "AppProc": np.percentile(traces["app_proc"], q=99) / 1000,
        "Network": np.percentile(traces["network"], q=99) / 1000,
        "EndToEnd": np.percentile(traces["end_to_end"], q=99) / 1000,
    }


with open(os.path.join(DATA_DIR, "result_p95.csv"), "wt") as f:
    writer = csv.writer(f)
    for service, lats in p95_latency.items():
        for cat, lat in lats.items():
            writer.writerow([service, cat, lat])

with open(os.path.join(DATA_DIR, "result_p99.csv"), "wt") as f:
    writer = csv.writer(f)
    for service, lats in p99_latency.items():
        for cat, lat in lats.items():
            writer.writerow([service, cat, lat])

