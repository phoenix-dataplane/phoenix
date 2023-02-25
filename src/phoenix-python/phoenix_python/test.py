from phoenix_python import shmservice

def main():
    a = shmservice.Hint(shmservice.Mode.Dedicate)

    shmservice.shm_register("/tmp/phoenix","control.sock","Salloc",a)

if __name__ == "__main__":
    main()