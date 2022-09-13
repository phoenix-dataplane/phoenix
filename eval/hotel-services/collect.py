import subprocess
import os

GEO_SERVER = "danyang-06"
RATE_SERVER = "danyang-06"
PROFILE_SERVER = "danyang-05"
SEARCH_SERVER = "danyang-04"
FRONTEND_SERVER = "danyang-03"

os.makedirs("/tmp/mrpc-eval/microservices", exist_ok=True)
os.makedirs(os.path.expanduser("~/nfs/mrpc-microservices-eval"), exist_ok=True)
COLLECT_GEO = "rsync -avz {}:/tmp/mrpc-eval/microservices/geo.csv /tmp/mrpc-eval/microservices/geo.csv".format(GEO_SERVER)
COLLECT_RATE = "rsync -avz {}:/tmp/mrpc-eval/microservices/rate.csv /tmp/mrpc-eval/microservices/rate.csv".format(RATE_SERVER)
COLLECT_PROFILE = "rsync -avz {}:/tmp/mrpc-eval/microservices/profile.csv /tmp/mrpc-eval/microservices/profile.csv".format(PROFILE_SERVER)
COLLECT_SEARCH = "rsync -avz {}:/tmp/mrpc-eval/microservices/search.csv /tmp/mrpc-eval/microservices/search.csv".format(SEARCH_SERVER)
COLLECT_FRONTEND = "rsync -avz {}:/tmp/mrpc-eval/microservices/frontend.csv /tmp/mrpc-eval/microservices/frontend.csv".format(FRONTEND_SERVER)

BACKUP = "cp /tmp/mrpc-eval/microservices/*.csv ~/nfs/mrpc-microservices-eval/."

subprocess.run(COLLECT_GEO, shell=True)
subprocess.run(COLLECT_RATE, shell=True)
subprocess.run(COLLECT_PROFILE, shell=True)
subprocess.run(COLLECT_SEARCH, shell=True)
subprocess.run(COLLECT_FRONTEND, shell=True)
subprocess.run(BACKUP, shell=True)
