#include <glog/logging.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <rte_mbuf_core.h>
#include <stdint.h>
#include <stdlib.h>

const uint32_t kNumMBufs = 8192;
const uint32_t kMBufCacheSize = 256;

const uint16_t kNumRxQueues = 1;
const uint16_t kNumTxQueues = 1;
const uint16_t kPortId = 0;

const uint16_t kRxQueueSize = 128;
const uint16_t kTxQueueSize = 128;

const size_t kMaxBurstSize = 32;

int init_port(struct rte_mempool* mbuf_pool, uint16_t port_id) {
  int rc;
  struct rte_eth_conf eth_conf;
  memset(&eth_conf, 0, sizeof(eth_conf));
  eth_conf.rxmode.max_rx_pkt_len = RTE_ETHER_MAX_LEN;

  auto nb_rx_queue = kNumRxQueues;
  auto nb_tx_queue = kNumTxQueues;
  // eth dev configure
  rc = rte_eth_dev_configure(port_id, nb_rx_queue, nb_tx_queue, &eth_conf);
  if (rc != 0) return rc;

  // setup rx queues
  auto eth_dev_socket_id = rte_eth_dev_socket_id(port_id);
  for (auto i = 0; i < nb_rx_queue; i++) {
    rc = rte_eth_rx_queue_setup(port_id, i, kRxQueueSize, eth_dev_socket_id,
                                nullptr, mbuf_pool);
    if (rc < 0) return rc;
  }

  // setup tx queues
  for (auto i = 0; i < nb_rx_queue; i++) {
    rc = rte_eth_tx_queue_setup(port_id, i, kTxQueueSize, eth_dev_socket_id,
                                nullptr);
    if (rc < 0) return rc;
  }

  // start device
  return rte_eth_dev_start(port_id);
}

int lcore_main(void* args) {
  unsigned lcore_id;
  lcore_id = rte_lcore_id();
  LOG(INFO) << "lcore_main from core " << lcore_id;

  struct rte_mempool* mbuf_pool = static_cast<struct rte_mempool*>(args);

  // 23:46:38.917967 IP (tos 0x0, ttl 64, id 48800, offset 0, flags [DF], proto
  // ICMP (1), length 84)
  //   rdma0.danyang-02.cs.duke.edu > rdma0.danyang-05.cs.duke.edu: ICMP echo
  //   request, id 9, seq 1, length 64
  // 0x0000:  0c42 a1ef 1b06 0c42 a1ef 1b26 0800 4500  .B.....B...&..E.
  // 0x0010:  0054 bea0 4000 4001 53f2 c0a8 d322 c0a8  .T..@.@.S...."..
  // 0x0020:  d3a2 0800 aad7 0009 0001 1eea f860 0000  .............`..
  // 0x0030:  0000 6900 0e00 0000 0000 1011 1213 1415  ..i.............
  // 0x0040:  1617 1819 1a1b 1c1d 1e1f 2021 2223 2425  ...........!"#$%
  // 0x0050:  2627 2829 2a2b 2c2d 2e2f 3031 3233 3435  &'()*+,-./012345
  // 0x0060:  3637
  static const uint8_t icmp_req[] = {
      0x0c, 0x42, 0xa1, 0xef, 0x1b, 0x06, 0x0c, 0x42, 0xa1, 0xef, 0x1b,
      0x26, 0x08, 0x00, 0x45, 0x00, 0x00, 0x54, 0xbe, 0xa0, 0x40, 0x00,
      0x40, 0x01, 0x53, 0xf2, 0xc0, 0xa8, 0xd3, 0x22, 0xc0, 0xa8, 0xd3,
      0xa2, 0x08, 0x00, 0xaa, 0xd7, 0x00, 0x09, 0x00, 0x01, 0x1e, 0xea,
      0xf8, 0x60, 0x00, 0x00, 0x00, 0x00, 0x69, 0x00, 0x0e, 0x00, 0x00,
      0x00, 0x00, 0x00, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
      0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22,
      0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d,
      0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37};
  size_t icmp_req_len = sizeof(icmp_req);

  struct rte_mbuf* pkts_burst[kMaxBurstSize];

  for (int i = 0; i < 10; i++) {
    for (auto& m : pkts_burst) {
      m = rte_pktmbuf_alloc(mbuf_pool);
      uint8_t* data = rte_pktmbuf_mtod(m, uint8_t*);
      memcpy(data, icmp_req, icmp_req_len);
      m->data_len = icmp_req_len;
      m->pkt_len = icmp_req_len;
    };

    uint16_t port_id = 0;
    // RTE_ETH_FOREACH_DEV(port_id) {
    uint16_t nb_tx = rte_eth_tx_burst(port_id, 0, pkts_burst, kMaxBurstSize);
    // }

    for (auto m : pkts_burst) {
      rte_pktmbuf_free(m);
    }
    printf("iter = %d\n", i);
  }

  return 0;
}

int main(int argc, char* argv[]) {
  google::InitGoogleLogging(argv[0]);

  LOG(INFO) << "initialize EAL";

  CHECK_GE(rte_eal_init(argc, argv), 0) << "Cannot init EAL";

  // create the mbuf pool
  LOG(INFO) << "create the mbuf pool";
  struct rte_mempool* mbuf_pool =
      rte_pktmbuf_pool_create("mbuf_pool", kNumMBufs, kMBufCacheSize, 0,
                              RTE_MBUF_DEFAULT_BUF_SIZE, rte_socket_id());

  CHECK(mbuf_pool) << "Cannot create mbuf pool";

  // initialize each port
  LOG(INFO) << "initialize each port";
  uint16_t port_id = 0;
  // RTE_ETH_FOREACH_DEV(port_id) {
  int err = init_port(mbuf_pool, port_id);
  CHECK(err == 0) << "Cannot init port, err: " << err
                  << ", port_id: " << port_id;
  // }

  // call lcore_main() on every worker lcore
  rte_eal_mp_remote_launch(lcore_main, mbuf_pool, CALL_MAIN);

  LOG(INFO) << "wait lcores to finish";
  rte_eal_mp_wait_lcore();
  return 0;
}
