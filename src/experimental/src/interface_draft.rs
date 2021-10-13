// Device Operations
pub trait DeviceOp {
    fn get_device_list() -> Result<Devices>;
    fn free_device_list(&mut self) -> Result<()>;
    fn get_device_name(&self) -> Result<String>;
    fn get_device_guid(&self) -> Result<Guid>;
    fn open_device(&self) -> Result<&dyn ContextOp>;
    fn close_device(ctx: &dyn ContextOp) -> Result<()>;
}

// Verbs Context Operations
pub trait ContextOp {
    fn query_device(&self) -> Result<DeviceAttr>;
    fn query_port(&self, port_num: u8) -> Result<PortAttr>;
    fn query_gid(&self, port_num: u8, index: i32) -> Result<Gid>;
    fn query_pkey(&self, port_num: u8, index: i32) -> Result<Pkey>;
    fn alloc_pd(&self) -> Result<&dyn ProtectionDomainOp>;
    fn dealloc_pd(pd: &dyn ProtectionDomainOp) -> Result<()>;
    fn create_cq(
        &self,
        cqe: i32,
        cq_context: *const (),
        channel: &CompChannel,
        comp_vector: i32,
    ) -> Result<CompletionQueue>;
    fn resize_cq(cq: &CompletionQueue, cqe: i32) -> Result<()>;
    fn destroy_cq(cq: &CompletionQueue) -> Result<()>;
    fn create_comp_channel(&self) -> Result<CompChannel>;
    fn destroy_comp_channel(channel: &CompChannel) -> Result<()>;
}

// Protection Domain Operations
pub trait ProtectionDomainOp {
    fn reg_mr(&self, addr: *const (), length: usize, flags: AccessFlags) -> Result<MemoryRegion>;
    fn dereg_mr(mr: &MemoryRegion) -> Result<()>;
    fn create_qp(&self, qp_init_attr: QpInitAttr) -> Result<QueuePair>;
    fn destroy_qp(qp: &QueuePair) -> Result<QueuePair>;
    // srq, xrc
    fn create_ah(&self, attr: &AhAttr) -> Result<AddressHandle>;
    fn destroy_ah(ah: &AddressHandle) -> Result<()>;
}

// Queue Pair Bringup
pub trait ConnectionManagement {
    fn modify_qp(&mut self, attr: &QpAttr, attr_mask: QpAttrMask) -> Result<()>;
    // bind
    // listen
    // resolve_addr
    // resolve_route
    // connect
    // disconnect
}

// Active Queue pair Operations
pub trait Transport {
    fn query_qp(&self, attr_mask: QpAttrMask) -> Result<(QpAttr, QpInitAttr)>;
    // query_srq, xrc
    fn post_recv(&self, wr: &RecvWorkRequest) -> Result<()>;
    fn post_send(&self, wr: &SendWorkRequest) -> Result<()>;
    fn req_notify_cq(cq: &CompletionQueue, solicited_only: i32) -> Result<()>;
    fn get_cq_event(channel: &CompChannel) -> Result<&CompletionQueue>;
    fn ack_cq_event(cq: &CompletionQueue, nevents: u32);
    fn poll_cq(cq: &CompletionQueue, num_entries: i32) -> Result<WorkCompletionIter>;
    fn init_ah_from_wc(
        ctx: &ContextOp,
        port_num: u8,
        wc: &WorkCompletion,
        grh: &Grh,
    ) -> Result<AhAttr>;
    fn create_ah_from_wc(
        pd: &ProtectionDomainOp,
        wc: &WorkCompletion,
        grh: &Grh,
        port_num: u8,
    ) -> Result<AhAttr>;
    // mcast
}

// Event Handling Operations
pub trait EventNotification {
    fn get_async_event(ctx: &ContextOp) -> Result<&AsyncEvent>;
    fn ack_async_event(event: &mut AsyncEvent);
    fn event_type_str(event_type: EventType) -> &'static str;
}
