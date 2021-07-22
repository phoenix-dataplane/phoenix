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

  for (;;) {
    struct rte_mbuf* pkts_burst[kMaxBurstSize];

    for (auto& m : pkts_burst) {
      m = rte_pktmbuf_alloc(mbuf_pool);
    };

    uint16_t port_id;
    // TODO(cjr): change this to stick to one port for a thread
    RTE_ETH_FOREACH_DEV(port_id) {
      uint16_t nb_rx = rte_eth_rx_burst(port_id, 0, pkts_burst, kMaxBurstSize);
      if (nb_rx == 0) continue;

      for (size_t i = 0; i < nb_rx; i++) {
        auto eth = rte_pktmbuf_mtod(pkts_burst[i], struct rte_ether_hdr*);
        auto& smac = eth->s_addr.addr_bytes;
        printf(
            "receive a packet from %02x:%02x:%02x:%02x:%02x:%02x, length: %d\n",
            smac[0], smac[1], smac[2], smac[3], smac[4], smac[5],
            pkts_burst[i]->pkt_len);
      }
    }

    for (auto m : pkts_burst) {
      rte_pktmbuf_free(m);
    }
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
  uint16_t port_id;
  RTE_ETH_FOREACH_DEV(port_id) {
    int err = init_port(mbuf_pool, port_id);
    CHECK(err == 0) << "Cannot init port, err: " << err
                    << ", port_id: " << port_id;
  }

  // call lcore_main() on every worker lcore
  rte_eal_mp_remote_launch(lcore_main, mbuf_pool, CALL_MAIN);

  LOG(INFO) << "wait lcores to finish";
  rte_eal_mp_wait_lcore();
  return 0;
}
