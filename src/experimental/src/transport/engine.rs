use std::collections::HashMap;

use interface::Handle;
use ipc;
use ipc::cmd::{Request, Response};

use engine::{Engine, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

/// A variety of tables map a `Handle` to a kind of RNIC resource.
#[derive(Default)]
struct Resource<'ctx> {
    handle_cnt: usize,
    pd_table: HashMap<Handle, ibv::ProtectionDomain<'ctx>>,
    cq_table: HashMap<Handle, ibv::CompletionQueue<'ctx>>,
    cmid_table: HashMap<Handle, CmId>,
    mr_table: HashMap<Handle, rdmacm::MemoryRegion>,
}

impl<'ctx> Resource<'ctx> {
    fn new() -> Self {
        Default::default()
    }

    fn allocate_handle(&mut self) -> Handle {
        self.handle_cnt += 1;
        Handle(self.handle_cnt)
    }
}

pub struct TransportEngine<'ctx> {
    tx: ipc::Sender<Response>,
    rx: ipc::Receiver<Request>,
    mode: SchedulingMode,

    resource: Resource<'ctx>,
}

impl<'ctx> TransportEngine<'ctx> {
    pub fn new(
        tx: ipc::Sender<Response>,
        rx: ipc::Receiver<Request>,
        mode: SchedulingMode,
    ) -> Self {
        TransportEngine {
            tx,
            rx,
            mode,
            resource: Resource::new(),
        }
    }
}

impl<'ctx> Upgradable for TransportEngine<'ctx> {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }

    fn resume(&mut self) {
        unimplemented!();
    }
}

impl<'ctx> Engine for TransportEngine<'ctx> {
    fn init(&mut self) {
        unimplemented!();
    }

    fn run(&mut self) -> bool {
        match self.rx.try_recv() {
            // handle request
            Ok(req) => {
                match req {
                    Request::NewClient(..) => unreachable!(),
                    Request::Hello(number) => {
                        self.tx.send(Response::HelloBack(number)).unwrap();
                    }
                    Request::GetAddrInfo(node, service, hints) => {
                        trace!(
                            "node: {:?}, service: {:?}, hints: {:?}",
                            node,
                            service,
                            hints,
                        );
                        let hints = hints.map(rdmacm::AddrInfoHints::from);
                        let ai = rdmacm::AddrInfo::getaddrinfo(
                            node.as_deref(),
                            service.as_deref(),
                            hints.as_ref(),
                        );
                        let ret = ai.map_or_else(
                            |e| Err(interface::Error::GetAddrInfo(e.raw_os_error().unwrap())),
                            |ai| Ok(ai.into()),
                        );
                        self.tx.send(Response::GetAddrInfo(ret)).unwrap();
                    }
                    Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                        // do something with this
                        trace!(
                            "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                            ai,
                            pd_handle,
                            qp_init_attr
                        );

                        let pd = pd_handle.and_then(|h| self.resource.pd_table.get(&h));
                        let qp_init_attr = qp_init_attr.map(|a| {
                            let attr = ibv::QpInitAttr {
                                qp_context: 0,
                                send_cq: a.send_cq.and_then(|h| self.resource.cq_table.get(&h.0)),
                                recv_cq: a.recv_cq.and_then(|h| self.resource.cq_table.get(&h.0)),
                                cap: a.cap.into(),
                                qp_type: a.qp_type.into(),
                                sq_sig_all: a.sq_sig_all,
                            };
                            attr.to_ibv_qp_init_attr()
                        });

                        let ret = match CmId::create_ep(&ai.into(), pd, qp_init_attr.as_ref()) {
                            Ok(cmid) => {
                                let cmid_handle = self.resource.allocate_handle();
                                self.resource
                                    .cmid_table
                                    .insert(cmid_handle, cmid)
                                    .ok_or(())
                                    .unwrap_err();
                                Ok(cmid_handle)
                            }
                            Err(e) => Err(interface::Error::RdmaCm(e.raw_os_error().unwrap())),
                        };

                        self.tx.send(Response::CreateEp(ret)).unwrap();
                    }
                    Request::Listen(cmid_handle, backlog) => {
                        trace!("cmid_handle: {:?}, backlog: {}", cmid_handle, backlog);
                        let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                            Err(interface::Error::NotFound),
                            |listener| {
                                listener.listen(backlog).map_err(|e| {
                                    interface::Error::RdmaCm(e.raw_os_error().unwrap())
                                })
                            },
                        );
                        self.tx.send(Response::Listen(ret)).unwrap()
                    }
                    Request::Accept(cmid_handle, conn_param) => {
                        trace!(
                            "cmid_handle: {:?}, conn_param: {:?}",
                            cmid_handle,
                            conn_param
                        );
                        warn!("TODO: conn_param is ignored for now");
                        let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                            Err(interface::Error::NotFound),
                            |listener| {
                                listener.accept().map_err(|e| {
                                    interface::Error::RdmaCm(e.raw_os_error().unwrap())
                                })
                            },
                        );
                        self.tx.send(Response::Accept(ret)).unwrap()
                    }
                    Request::GetRequest(cmid_handle) => {
                        trace!("cmid_handle: {:?}", cmid_handle);

                        let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                            Err(interface::Error::NotFound),
                            |listener| {
                                listener.get_request().map_err(|e| {
                                    interface::Error::RdmaCm(e.raw_os_error().unwrap())
                                })
                            },
                        );
                        let ret = ret.map(|new_cmid| {
                            let new_handle = self.resource.allocate_handle();
                            self.resource
                                .cmid_table
                                .insert(new_handle, new_cmid)
                                .ok_or(())
                                .unwrap_err();
                            new_handle
                        });
                        self.tx.send(Response::GetRequest(ret)).unwrap()
                    }
                    Request::Connect(cmid_handle, conn_param) => {
                        trace!(
                            "cmid_handle: {:?}, conn_param: {:?}",
                            cmid_handle,
                            conn_param
                        );
                        warn!("TODO: conn_param is ignored for now");
                        let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                            Err(interface::Error::NotFound),
                            |cmid| {
                                cmid.connect().map_err(|e| {
                                    interface::Error::RdmaCm(e.raw_os_error().unwrap())
                                })
                            },
                        );
                        self.tx.send(Response::Connect(ret)).unwrap()
                    }
                    Request::RegMsgs(cmid_handle, addr_range) => {
                        trace!("cmid_handle: {:?}, addr_range: {:?}", cmid_handle, addr_range);
                        let ret = self.resource.cmid_table.get(&cmid_handle).map_or(
                            Err(interface::Error::NotFound),
                            |listener| {
                                listener.reg_msgs().map_err(|e| {
                                    interface::Error::RdmaCm(e.raw_os_error().unwrap())
                                })
                            },
                        );
                        let ret = ret.map(|mr| {
                            let new_mr = self.resource.allocate_handle();
                            self.resource
                                .mr_table
                                .insert(new_mr, mr)
                                .ok_or(())
                                .unwrap_err();
                            new_mr
                        });
                        self.tx.send(Response::RegMsgs(ret)).unwrap()
                    }
                    _ => {
                        unimplemented!()
                    }
                }
                true
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                false
            }
            Err(ipc::TryRecvError::IpcError(e)) => {
                if matches!(e, ipc::IpcError::Disconnected) {
                    return true;
                }
                panic!("recv error: {:?}", e);
            }
        }
    }

    fn shutdown(&mut self) {
        unimplemented!();
    }

    fn enqueue(&self) {
        unimplemented!();
    }

    fn check_queue_len(&self) {
        unimplemented!();
    }
}
