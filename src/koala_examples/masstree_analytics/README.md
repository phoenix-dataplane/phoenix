# masstree analytics

`bash
git submodule update --recursive --init
`

`masstree-beta` may require huge pages to work. You can use the
usertools bundled with DPDK source code to setup huge pages on Linux.
(no need to actually install DPDK.)
```
sudo dpdk-stable-20.11.1/usertools/dpdk-hugepages.py -p 2M --setup 2G
```
