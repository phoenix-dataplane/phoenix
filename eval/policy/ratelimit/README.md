This folder contains scripts for rate limit policy.

1. Start receiver backend on `danyang-06`. Start sender backend on
   `danyang-05`.
```
cjr@danyang-06 $ ./start_receiver_koala.sh
cjr@danyang-05 $ ./start_sender_koala.sh
```

2. Start traffic.
```
./start_traffic.sh
```

3. Set the rate limit on the sender machine.
```
cjr@danyang-05 $ ./adjust_rate_figure_1.sh
```

4. Collect and parse results. This will read into `/tmp/mrpc-eval` and
   parse the text output to csv for plotting figures.
```
./collect.sh
```
