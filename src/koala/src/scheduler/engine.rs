use interface::engine::SchedulingMode;
use interface::rpc::{ImmFlags, RpcId, TransportStatus};
use interface::Handle;
use minstant::Instant;
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::{mem, ptr};
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::atomic::AtomicU32;
use std::time::Duration;
use anyhow::anyhow;
use futures::future::BoxFuture;
use ipc::RawRdmaMsgTx;
use crate::engine::datapath::{DataPathNode, EngineRxMessage, EngineTxMessage, RxOQueue, TryRecvError};
use crate::engine::{Decompose, Engine, EngineResult, future, Indicator, Vertex};
use crate::{log, tracing};
use crate::storage::{ResourceCollection, SharedStorage};
use rdma::POST_BUF_LEN;
use crate::transport_rdma::ops::Ops;
use crate::transport_rdma::DatapathError;

#[derive(Hash, PartialEq, Eq, Clone)]
struct FlattenKey(u64);

#[allow(unused)]
impl FlattenKey {
    fn new(ops: &Ops, handle: &Handle) -> Self {
        FlattenKey {
            0: (ops.id as u64) << 32 | handle.0 as u64,
        }
    }
}

impl Debug for FlattenKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FlattenKey [ops: {}; handle: {}]",
            (self.0 >> 32),
            (self.0 & 0xffffffff)
        )
    }
}

const MAX_INTERVAL_TIME: Duration = Duration::new(0, 5e3 as u32);


struct PolicyState {
    ops: &'static Ops,
    handle: Handle,
    last_timestamp: Instant,
    buffered_length: u64,
    local_buffer: VecDeque<RawRdmaMsgTx>,
    pub(crate) imm_send_flag: bool,
}

#[allow(unused)]
impl PolicyState {
    #[inline(always)]
    fn should_send(&self) -> bool {
        return self.buffered_length > 4096
            || self.local_buffer.len() > 3
            || (self.local_buffer.len() > 0
            && (self.imm_send_flag || self.last_timestamp.elapsed().ge(&MAX_INTERVAL_TIME)));
    }

    #[inline(always)]
    fn push_packet(&mut self, rpc_pkt: RawRdmaMsgTx) {
        if self.local_buffer.len() == 0 {
            self.last_timestamp = Instant::now();
        }
        self.buffered_length += rpc_pkt.range.len;
        self.local_buffer.push_back(rpc_pkt);
    }

    #[inline(always)]
    fn could_fuse(&self) -> bool {
        return self.buffered_length / (self.local_buffer.len() as u64) < 2048;
    }

    #[inline(always)]
    fn reset(&mut self) -> usize {
        let cnt = self.local_buffer.len();
        // todo: unsafe set_len(0)
        self.local_buffer.clear();
        self.buffered_length = 0;
        self.imm_send_flag = false;
        cnt
    }

    fn new(ops: &'static Ops, handle: Handle) -> PolicyState {
        PolicyState {
            ops,
            handle,
            last_timestamp: Instant::now(),
            buffered_length: 0,
            local_buffer: VecDeque::new(),
            imm_send_flag: false,
        }
    }
}

pub struct MetaPool {
    // queue[idx]: the set of allocated buffers belong to the idx
    pub(crate) queue: Vec<Vec<usize>>,
    pub(crate) counter: Vec<u32>,
    pub(crate) free: Vec<usize>,
}

/// Example:
/// let pool = MetaPool::new(32);
/// let handle = pool.alloc();
/// pool.insert(handle,1);
/// pool.insert(handle,2);
/// pool.free_one(handle); // None
/// let arr = pool.free_one(handle); // Some
/// // release arr...
/// pool.clear(handle); // now handle is invalid
impl MetaPool {
    fn new(len: usize) -> Self {
        let mut buf = Vec::with_capacity(len);
        buf.resize(len, Vec::with_capacity(8));
        let mut counter = Vec::with_capacity(len);
        counter.resize(len, 0);
        let mut free = Vec::with_capacity(len);
        for i in 0..len {
            free.push(i);
        }
        MetaPool {
            queue: buf,
            counter,
            free,
        }
    }

    fn alloc(&mut self) -> usize {
        // should not fail
        self.free.pop().unwrap()
    }

    fn insert(&mut self, handle: usize, entry: usize) {
        self.queue.get_mut(handle).unwrap().push(entry);
        *self.counter.get_mut(handle).unwrap() += 1;
    }

    /// When returning Some,it's ok to free them.
    fn free_one(&mut self, handle: usize) -> Option<&Vec<usize>> {
        let v = self.counter.get_mut(handle).unwrap();
        *v -= 1;
        if *v == 0 {
            self.queue.get(handle)
        } else {
            None
        }
    }

    fn clear(&mut self, handle: usize) {
        self.queue.get_mut(handle).unwrap().clear();
        self.free.push(handle);
    }
}

/// Some mechanics:
/// - When will pool free buffers: all buffers from the same rpc are acked
pub struct BufferPool {
    pub(crate) buf: Vec<BufferPage>,
    pub(crate) free: Vec<usize>,
    pub(crate) meta_pool: MetaPool,
    /// processed rpc_id -> handle of meta_pool
    pub(crate) mapping: HashMap<u64, usize>,
}

impl BufferPool {
    fn new(len: usize) -> Self {
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push(BufferPage::new());
        }
        let mut free = Vec::with_capacity(len);
        for i in 0..len {
            free.push(i);
        }
        BufferPool {
            buf,
            free,
            meta_pool: MetaPool::new(len),
            mapping: HashMap::new(),
        }
    }

    fn get_page(&mut self, id: RpcId) -> Option<(usize, &mut BufferPage)> {
        let k = id.encode_u64_without_flags();
        let free_page_handle = match self.free.pop() {
            Some(idx) => idx,
            None => {
                return None;
            }
        };
        let meta_handle = self
            .mapping
            .entry(k)
            .or_insert_with(|| self.meta_pool.alloc());
        self.meta_pool.insert(meta_handle.clone(), free_page_handle);
        self.at(free_page_handle).reset();
        Some((free_page_handle, self.at(free_page_handle)))
    }

    fn release_page(&mut self, id: RpcId, ack_queue: &mut RxOQueue) {
        let key = id.encode_u64_without_flags();
        match self.mapping.get(&key) {
            Some(meta_handle) => {
                if let Some(free_list) = self.meta_pool.free_one(*meta_handle) {
                    for ele in free_list {
                        for ack_rpc in self.buf.get_mut(*ele).unwrap().associated_rpcs() {
                            ack_queue
                                .send(EngineRxMessage::Ack(
                                    ack_rpc.clone_without_flags(),
                                    TransportStatus::Success,
                                ))
                                .unwrap();
                        }
                        self.free.push(*ele);
                    }
                    self.meta_pool.clear(*meta_handle);
                    self.mapping.remove(&key);
                }
            }
            None => {
                log::warn!("BufferPool: release missing key {}", id.encode_u64());
            }
        }
    }

    fn get_range_handle(&self, handle: usize) -> ipc::buf::Range {
        ipc::buf::Range {
            offset: unsafe { self.buf.as_ptr().offset(handle as isize) } as u64,
            len: PAGE_SIZE as u64,
        }
    }

    fn at(&mut self, handle: usize) -> &mut BufferPage {
        self.buf.get_mut(handle).unwrap()
    }
}

pub struct SchedulerEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) _mode: SchedulingMode,
    // without requirement for sychronization, we do not use Mutex wrapper
    policy_state: HashMap<FlattenKey, PolicyState>,
    pub(crate) buffer_pool: BufferPool,
    //     counter: usize,
    //     recording: StackedBuffer<(u32,u32),128>
}

// Manually implemented because of namespace.
impl Vertex for SchedulerEngine {
    #[inline]
    fn tx_inputs(&mut self) -> &mut Vec<crate::engine::datapath::graph::TxIQueue> {
        &mut self.node.tx_inputs
    }
    fn tx_outputs(&mut self) -> &mut Vec<crate::engine::datapath::graph::TxOQueue> {
        &mut self.node.tx_outputs
    }
    fn rx_inputs(&mut self) -> &mut Vec<crate::engine::datapath::graph::RxIQueue> {
        &mut self.node.rx_inputs
    }
    fn rx_outputs(&mut self) -> &mut Vec<crate::engine::datapath::graph::RxOQueue> {
        &mut self.node.rx_outputs
    }
}

impl Decompose for SchedulerEngine {
    fn flush(&mut self) -> anyhow::Result<()> {
        while !self.tx_inputs()[0].is_empty() {
            self.check_input_queue()?;
        }
        Ok(())
    }

    fn decompose(self: Box<Self>, _shared: &mut SharedStorage, _global: &mut ResourceCollection) -> (ResourceCollection, DataPathNode) {
        let mut engine = *self;

        let mut collections = ResourceCollection::with_capacity(5);
        tracing::trace!("dumping Scheduler states...");

        let node = unsafe {
            collections.insert("mode".to_string(), Box::new(ptr::read(&engine._mode)));
            collections.insert("policy_state".to_string(), Box::new(ptr::read(&engine.policy_state)));
            collections.insert("buffer_pool".to_string(), Box::new(ptr::read(&engine.buffer_pool)));
            ptr::read(&engine.node)
        };
        mem::forget(engine);
        (collections, node)
    }
}

impl SchedulerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> anyhow::Result<Self> {
        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast::<SchedulingMode>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let policy_state = *local
            .remove("policy_state")
            .unwrap()
            .downcast::<HashMap<FlattenKey, PolicyState>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let buffer_pool = *local
            .remove("mode")
            .unwrap()
            .downcast::<BufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        Ok(SchedulerEngine {
            node,
            indicator: Default::default(),
            _mode: mode,
            policy_state,
            buffer_pool,
        })
    }
}

impl Engine for SchedulerEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        format!("SchedulerEngine, scheduling over rdma operations")
    }

    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;
use crate::engine::datapath::fusion_layout::{BufferPage, PAGE_SIZE};
use crate::scheduler::stacked_buffer::StackedBuffer;

#[allow(unused)]
static DEBUG_COUNTER: AtomicU32 = AtomicU32::new(0);

impl SchedulerEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut n_work = 0;
            let mut n_work2 = 0;
            // let mut timer = crate::timer::Timer::new();
            // post from local buffer to NIC
            // todo(xyc): find better way to iterate to reduce latency
            for (_, v) in self.policy_state.iter_mut() {
                // no need to loop
                match SchedulerEngine::post_queue(v, &mut self.buffer_pool)? {
                    Progress(n) => n_work += n,
                    Status::Disconnected => return Ok(()),
                }
            }
            // timer.tick();

            // store msg from previous engine to buffer
            match self.check_input_queue()? {
                Progress(n) => n_work2 += n,
                Status::Disconnected => return Ok(()),
            }
            // timer.tick();

            self.indicator.set_nwork(n_work + n_work2);
            // if n_work>0 || n_work2>0 {
            //     log::info!("Scheduler mainloop: {} {} {}",n_work,n_work2,timer);
            // }

            future::yield_now().await;
        }
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        let mut cnt = 0;
        let node = &mut self.node;
        let input_vec = &mut node.tx_inputs;
        let rx_output_vec = &mut node.rx_outputs;

        // todo(xyc): optimize brute-force polling
        for (idx, tx_input) in input_vec.iter_mut().enumerate() {
            // In this inside code block, (ops, handle) won't change.
            let mut local_buffer: StackedBuffer<RawRdmaMsgTx, { POST_BUF_LEN }> = StackedBuffer::new();
            let mut fetch_next: bool;
            // Safety: only when len>0 could we use these.
            let mut target_ops: &Ops = unsafe { MaybeUninit::uninit().assume_init() };
            let mut target_handle: Handle = unsafe { MaybeUninit::uninit().assume_init() };
            while local_buffer.len() < POST_BUF_LEN {
                fetch_next = match tx_input.try_recv() {
                    Ok(engine_msg) => match engine_msg {
                        EngineTxMessage::SchedMessage(ops_handle, handle, rpc_msg) => {
                            target_ops = unsafe { Ops::from_addr(ops_handle) };
                            target_handle = handle;
                            local_buffer.append(rpc_msg);
                            true
                        }
                        EngineTxMessage::ReclaimSchedFusedBuffer(id) => {
                            self.buffer_pool.release_page(id, &mut rx_output_vec[idx]);
                            true
                        }
                        EngineTxMessage::RpcMessage(_) => {
                            unreachable!()
                        }
                        EngineTxMessage::ReclaimRecvBuf(..) => {
                            // will here unreachable?
                            unreachable!()
                        }
                    },
                    Err(TryRecvError::Empty) => false,
                    Err(TryRecvError::Disconnected) => {
                        // todo(xyc): GC for disconnected engine
                        return Ok(Status::Disconnected);
                    }
                };
                if !fetch_next {
                    break;
                }
            }
            // immediately post buffer
            if local_buffer.len() > 0 {
                cnt += local_buffer.len();
                // detailed policy here
                let slice = local_buffer.as_slice();
                // if false{
                //     let x = DEBUG_COUNTER.load(Ordering::Relaxed);
                //     if x % 509 == 0 {
                //         let iter = slice.iter();
                //         log::info!("============ total: {}ele, {}bytes =========",iter.len(),
                // iter.map(|x|x.range.len).sum::<u64>());
                //         let iter = slice.iter();
                //         for (i, ele) in iter.enumerate() {
                //             log::info!("{}: {}",i,ele.range.len);
                //         }
                //     }
                //     DEBUG_COUNTER.store(if x >= 10000 { 0 } else { x + 1 }, Ordering::Relaxed);
                // }

                if (slice.iter().map(|x| x.range.len).sum::<u64>() as usize / slice.len()) < 2048 {
                    // fusion
                    Self::_post_queue_fused(
                        target_ops,
                        target_handle.clone(),
                        slice.iter(),
                        &mut self.buffer_pool,
                    )?;
                } else {
                    unsafe { target_ops.post_send_batch(target_handle.clone(), slice.iter())? }
                }
            }
        }
        Ok(Progress(cnt))
    }

    fn post_queue(
        state: &mut PolicyState,
        buffer_pool: &mut BufferPool,
    ) -> Result<Status, DatapathError> {
        if state.should_send() {
            assert!(state.local_buffer.len() < POST_BUF_LEN);
            return if state.could_fuse() {
                // fuse msgs to buffer
                Self::_post_queue_fused(
                    state.ops,
                    state.handle,
                    state.local_buffer.iter(),
                    buffer_pool,
                )?;
                Ok(Progress(state.reset()))
            } else {
                let iter = state.local_buffer.range(0..state.local_buffer.len());
                unsafe {
                    state.ops.post_send_batch(state.handle, iter)?;
                }
                Ok(Progress(state.reset()))
            };
        }
        Ok(Progress(0))
    }
}

enum FusingState<'a> {
    FreeStart,
    /// Avoid copy when holding one but no second
    HoldingOne {
        prev: &'a RawRdmaMsgTx,
        prev_is_ending: bool,
    },
    /// Handle of buffer page
    Fusing {
        handle: usize,
        prev_is_ending: bool,
    },
    Exhausted,
}

use FusingState::{FreeStart, HoldingOne, Fusing, Exhausted};
use crate::envelop::ResourceDowncast;
use crate::module::{ModuleCollection, Version};

impl SchedulerEngine {
    fn _post_queue_fused<'a, I>(
        ops: &Ops,
        handle: Handle,
        iter: I,
        buffer_pool: &mut BufferPool,
    ) -> Result<(), DatapathError>
        where
            I: ExactSizeIterator<Item=&'a RawRdmaMsgTx>,
    {
        let mut temp_arr: [MaybeUninit<RawRdmaMsgTx>; POST_BUF_LEN] =
            unsafe { MaybeUninit::uninit().assume_init() };
        let mut cursor = 0; // current available index
        let mut fusing_state: FusingState = FreeStart;
        // let mut debug_buffer: [char; POST_BUF_LEN+2] = [' '; POST_BUF_LEN+2];
        // let mut debug_counter = 0;
        for ele in iter {
            // debug_buffer[debug_counter] = match fusing_state {
            //     FreeStart => 'S',
            //     HoldingOne{prev,prev_is_ending} => if prev_is_ending{'h'}else{ 'H'},
            //     Fusing{handle,prev_is_ending} => if prev_is_ending{'x'}else{'X'},
            //     Exhausted => 'E',
            // };
            // debug_counter += 1;
            fusing_state = match fusing_state {
                FreeStart => HoldingOne {
                    prev: ele,
                    prev_is_ending: ele.has_all(ImmFlags::RPC_ENDING),
                },
                HoldingOne {
                    prev,
                    prev_is_ending,
                } => {
                    assert_eq!(prev.has_all(ImmFlags::RPC_ENDING), prev_is_ending);
                    let ele_is_ending = ele.has_all(ImmFlags::RPC_ENDING);
                    if BufferPage::safe_to_hold_two(
                        (prev.range.len + ele.range.len) as usize,
                        prev_is_ending as u8 + ele_is_ending as u8,
                    ) {
                        let mut rpc_id = prev.rpc_id;
                        if let Some((handle, buffer)) = buffer_pool.get_page(rpc_id) {
                            rpc_id.flag_bits.set(if prev_is_ending {
                                ImmFlags::FUSE_PKT | ImmFlags::FUSE_RPC
                            } else {
                                ImmFlags::FUSE_PKT
                            });
                            temp_arr[cursor].write(RawRdmaMsgTx {
                                mr: prev.mr,
                                range: ipc::buf::Range { offset: 0, len: 1 }, // should be examined later
                                rpc_id,
                            });
                            buffer.copy_in(
                                prev.range,
                                if prev_is_ending {
                                    Some(prev.rpc_id)
                                } else {
                                    None
                                },
                            );
                            buffer.copy_in(
                                ele.range,
                                if ele_is_ending {
                                    Some(ele.rpc_id)
                                } else {
                                    None
                                },
                            );
                            if ele_is_ending {
                                unsafe { temp_arr[cursor].assume_init_mut() }
                                    .set_flag(ImmFlags::RPC_ENDING);
                                Fusing {
                                    handle,
                                    prev_is_ending: true,
                                }
                            } else {
                                Fusing {
                                    handle,
                                    prev_is_ending: false,
                                }
                            }
                        } else {
                            // cannot allocate buffer
                            temp_arr[cursor].write(prev.clone());
                            temp_arr[cursor + 1].write(ele.clone());
                            cursor += 2;
                            Exhausted
                        }
                    } else {
                        // no space to hold two
                        temp_arr[cursor].write(prev.clone());
                        cursor += 1;
                        HoldingOne {
                            prev: ele,
                            prev_is_ending: ele_is_ending,
                        }
                    }
                }
                Fusing {
                    handle,
                    prev_is_ending,
                } => {
                    let buffer = buffer_pool.at(handle);
                    let ele_is_ending = ele.has_all(ImmFlags::RPC_ENDING);
                    if buffer.can_hold(ele.range.len as usize, ele_is_ending) {
                        buffer.copy_in(
                            ele.range,
                            if ele_is_ending {
                                Some(ele.rpc_id)
                            } else {
                                None
                            },
                        );
                        if prev_is_ending {
                            // need to set WR flags
                            unsafe { temp_arr[cursor].assume_init_mut() }
                                .set_flag(ImmFlags::FUSE_RPC);
                        }
                        if ele_is_ending {
                            unsafe { temp_arr[cursor].assume_init_mut() }
                                .set_flag(ImmFlags::RPC_ENDING);
                            Fusing {
                                handle,
                                prev_is_ending: true,
                            }
                        } else {
                            Fusing {
                                handle,
                                prev_is_ending: false,
                            }
                        }
                    } else {
                        unsafe { temp_arr[cursor].assume_init_mut() }.range = buffer.finish();
                        cursor += 1;
                        HoldingOne {
                            prev: ele,
                            prev_is_ending: ele_is_ending,
                        }
                    }
                }
                Exhausted => {
                    temp_arr[cursor].write(ele.clone());
                    cursor += 1;
                    Exhausted
                }
            }
        }
        // debug_buffer[debug_counter] = match fusing_state {
        //     FreeStart => 'S',
        //     HoldingOne{prev,prev_is_ending} => if prev_is_ending{'h'}else{ 'H'},
        //     Fusing{handle,prev_is_ending} => if prev_is_ending{'x'}else{'X'},
        //     Exhausted => 'E',
        // };
        // debug_counter += 1;

        // finish
        match fusing_state {
            FreeStart => {}
            HoldingOne {
                prev,
                prev_is_ending,
            } => {
                assert_eq!(prev.has_all(ImmFlags::RPC_ENDING), prev_is_ending);
                temp_arr[cursor].write(prev.clone());
                cursor += 1;
            }
            Fusing {
                handle,
                prev_is_ending: _,
            } => {
                let buffer = buffer_pool.at(handle);
                // Here, the last sge won't be the end of RPC
                unsafe { temp_arr[cursor].assume_init_mut() }.range = buffer.finish();
                cursor += 1;
            }
            Exhausted => {
                log::warn!("Scheduler Bufferpool exhanusted.");
            }
        }
        // if false{
        //     let x = DEBUG_COUNTER.load(Ordering::Relaxed);
        //     if x % 509 == 0 {
        //         log::info!("============ prev:{} now:{} =========",debug_counter,cursor);
        //         log::info!("{}",String::from_iter(&debug_buffer[0..debug_counter]));
        //         let iter = unsafe { mem::transmute::<_, &[RawRdmaMsgTx]>(&temp_arr[0..cursor]) }.iter();
        //         for (i, ele) in iter.enumerate() {
        //             log::info!("{}: {}",i,ele.range.len);
        //         }
        //     }
        //     DEBUG_COUNTER.store(if x >= 10000 { 0 } else { x + 1 }, Ordering::Relaxed);
        // }
        let iter = unsafe { mem::transmute::<_, &[RawRdmaMsgTx]>(&temp_arr[0..cursor]) }.iter();
        unsafe {
            ops.post_send_batch(handle.clone(), iter)?;
        }
        return Ok(());
    }
}

impl SchedulerEngine {
    pub(crate) fn new(node: DataPathNode, mode: SchedulingMode) -> Self {
        SchedulerEngine {
            node,
            indicator: Default::default(),
            _mode: mode,
            policy_state: Default::default(),
            buffer_pool: BufferPool::new(1024 * 1024 * 128 / PAGE_SIZE), // 128Mb
            // counter:0,
            // recording:StackedBuffer::new()
        }
    }
}
