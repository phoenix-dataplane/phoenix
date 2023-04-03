<div align="center">
<img src="https://github.com/phoenix-dataplane/phoenix/raw/main/phoenix-logo-red-black.png" alt="logo" width="250"></img>
<br></br>
</div>

# Introduction to Phoenix Project
Phoenix is a dataplane service which serves as a framework to develop and deploy various kinds of managed services.

The key features of Phoenix include:
- **Modular plugin systems**: engines can be developed as plugins, dynamically load into the Phoenix service at runtime,
and cam be live upgraded without distributing user applications.
- **Low-latency networking with kernel bypassing**:
Phoenix service can utilize userspace solutions like RDMA and DPDK for high throughput and low latency networking.
(DPDK support will be added in future releases.)
- **Policy management**: Phoenix provides support for application-level policies that infrastructure administers could
specify to control the behaviours and resources usages of user applications. 

## mRPC
mRPC, (stands for managed RPC service), is built on top of Phoenix as an experimental feature.
mRPC implements a novel RPC architecture that decouples marshalling/unmarshalling from tranditional RPC libraries into a centralized system service.

Compared to traditional library + sidecar solutions such as gRPC + Envoy,
mRPC applies network policies and observability features with both security and low performance overhead,
i.e., with minimal data movement and no redundant (un)marshalling. The mechanism supports live upgrade of
RPC bindings, policies, transports, and marshalling without disrupting running applications.
