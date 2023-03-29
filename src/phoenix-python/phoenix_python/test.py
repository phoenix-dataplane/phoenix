from phoenix_python import shmservice

def main():
    hint = shmservice.Hint(shmservice.Mode.Dedicate)
    shmservice.salloc_register("/tmp/phoenix_eric","control.sock")
    # shmservice.shm_register("/tmp/phoenix_eric","control.sock","Salloc",hint)
    print(shmservice.allocate_shm(20))
if __name__ == "__main__":
    main()