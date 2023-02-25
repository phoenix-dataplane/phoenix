from phoenix_python import shmservice
import argparse

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("-s", required = True)
    argument = parser.parse_args()
    service = argument.s

    if service == "salloc":
        hint = shmservice.Hint(shmservice.Mode.Dedicate)
        shmservice.shm_register("/tmp/phoenix","control.sock","Salloc",hint)
    elif service == "mrpc-tcp":
        hint = shmservice.Hint(shmservice.Mode.Dedicate)
        shmservice.shm_register("/tmp/phoenix","control.sock","Mrpc",hint,"{\"transport\":\"Tcp\",\"nic_index\":0,\"core_id\":null}")
    else:
        raise RuntimeError("Invalid service type requested")

if __name__ == "__main__":
    main()