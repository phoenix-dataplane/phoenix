This folder contains scripts for content-based-ACL policy.
**Edit config.toml and koala.toml first**

1. Start receiver backend on `danyang-06`. Start sender backend on
   `danyang-05`.
```
cjr@danyang-05 $ ./start_koala.sh [/tmp/mrpc-eval]
```

2. Start traffic
```
./start_traffic.sh [/tmp/mrpc-eval]
```

3. Collect and parse results. This will read into `/tmp/mrpc-eval` and
   parse the text output to csv for plotting figures.
```
python3 ./collect.py [/tmp/mrpc-eval]
```
