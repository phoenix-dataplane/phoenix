use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::slice::SliceIndex;

use libkoala::Error as LibKoalaError;
use libkoala::cm::CmIdBuilder;
use libkoala::cm::CmId;
use libkoala::verbs::{MemoryRegion, SendFlags, WorkCompletion, WcStatus};

use super::CommunicatorError;

const MAX_RECV_WR: u32 = 128;
const MAX_SEND_WR: u32 = 128;
const HANDSHAKE_MAGIC: u64 = 0xc2f1fb770118add9;

pub struct Communicator {
    my_index: usize,
    num_peers: usize,
    cm_ids: Vec<Option<CmId>>,
}

impl Communicator {
    pub fn new<A: ToSocketAddrs>(addrs: Vec<A>, my_index: usize) -> Result<Communicator, CommunicatorError> {

        let addrs = addrs.into_iter().map(|x: A| -> Result<SocketAddr, CommunicatorError> {
            x.to_socket_addrs()?.next().ok_or(CommunicatorError::NoAddrResolved)
        }).collect::<Result<Vec<_>, _>>()?;
        let num_peers = addrs.len();

        let my_addr = &addrs[my_index];
        let listener = CmIdBuilder::new()
            .set_max_recv_wr(MAX_RECV_WR)
            .set_max_send_wr(MAX_SEND_WR)
            .bind(my_addr)?;
            
        let mut accept_cm_ids = (0..(num_peers - my_index - 1)).map(|_| None).collect::<Vec<_>>();
        for _i in 0..(num_peers - my_index - 1) {
            let conn_request = listener.get_request()?;
            let pre_id = conn_request.build()?;

            let mut recv_mr = pre_id.alloc_msgs::<(u64, u32)>(1)?;
            unsafe {
                pre_id.post_recv(&mut recv_mr, .., 0)?;
            }
            
            let cm_id = pre_id.accept(None)?;
            // blocking
            let wc = cm_id.get_recv_comp()?;
            assert_eq!(wc.wr_id, 0);
            assert_eq!(wc.status, WcStatus::Success);
            if (wc.wr_id != 0) || (wc.status != WcStatus::Success) {
                return Err(CommunicatorError::HandshakeError("failed to receive handshake identifier"))
            }

            let identifier = recv_mr.as_slice().get(0).ok_or(
                CommunicatorError::MemoryRegionError("failed to read handshake identifier")
            )?;

            if identifier.0 != HANDSHAKE_MAGIC {
                return Err(CommunicatorError::HandshakeError("recevied incorrect handshake magic"));
            }
            let index = identifier.1 as usize;
            if my_index + 1 <= index && index < num_peers {
                eprintln!("accepted connection from worker {}", index);
                accept_cm_ids[index - my_index - 1]  = Some(cm_id);
            }
            else {
                return Err(CommunicatorError::HandshakeError("recevied invalid remote worker index"));
            }
        }
        
        let mut connect_cm_ids = Vec::with_capacity(my_index);
        for (idx, addr) in addrs.iter().take(my_index).enumerate() {
            let builder = CmIdBuilder::new()
                .set_max_recv_wr(MAX_RECV_WR)
                .set_max_send_wr(MAX_SEND_WR)
                .resolve_route(addr)?;
            
            println!("CMID BUILDER ADDR RESOLVED");


            let pre_id = builder.build()?;
            println!("PREID QP PAIR CREATED");

            let cm_id = pre_id.connect(None)?;
            println!("CONNECTION MADE");

            let mut send_mr = cm_id.alloc_msgs::<(u64, u32)>(1)?;
            let identifier = send_mr.first_mut()
                .ok_or(CommunicatorError::MemoryRegionError("failed to write handshake identifier to send_mr"))?;
            *identifier = (HANDSHAKE_MAGIC, my_index as u32);
            println!("CONNECTION MADE");
            cm_id.post_send(&send_mr, .., 0, SendFlags::SIGNALED)?;
            connect_cm_ids.push(Some(cm_id));           
            eprintln!("connecting to worker {}", idx);
        
        }

        for cmid in connect_cm_ids.iter() {
            if let Some(cmid) = cmid {
                let wc = cmid.get_send_comp()?;
                if (wc.wr_id != 0) || (wc.status != WcStatus::Success) {
                    return Err(CommunicatorError::HandshakeError("fail to send handshake identifier"));
                }
            }
        }

        let mut cm_ids = connect_cm_ids;
        cm_ids.push(None);
        cm_ids.extend(accept_cm_ids.into_iter());

        eprintln!("all connections established");

        Ok(Communicator {
            my_index,
            num_peers,
            cm_ids
        })
    }

    pub fn get_cmid(&self, index: usize) -> Result<&CmId, CommunicatorError> {
        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        Ok(cmid)
    }

    pub fn allocate_msgs_pair<T: Sized + Copy>(&self, index: usize, len: usize) -> Result<(MemoryRegion<T>, MemoryRegion<T>), CommunicatorError> {
        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        let send_mr = cmid.alloc_msgs(len)?;
        let recv_mr = cmid.alloc_msgs(len)?;
        Ok((send_mr, recv_mr))
    }

    pub fn allocate_msgs<T: Sized + Copy>(&self, index: usize, len: usize) -> Result<MemoryRegion<T>, CommunicatorError> {
        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        let mr = cmid.alloc_msgs(len)?;
        Ok(mr)
    }

    pub fn allocate_cluster_mr<'cm, T: Sized + Copy>(&'cm self, len: usize) -> Result<PeersSendRecvMR<'cm, T>, CommunicatorError> {
        let mut send_mrs = Vec::with_capacity(self.num_peers);
        let mut recv_mrs = Vec::with_capacity(self.num_peers);
        for cm_id in self.cm_ids.iter() {
            match cm_id {
                Some(cmid) => {
                    let send_mr = cmid.alloc_msgs(len)?;
                    let recv_mr = cmid.alloc_msgs(len)?;    
                    send_mrs.push(Some(send_mr));
                    recv_mrs.push(Some(recv_mr));
                },
                None => {
                    send_mrs.push(None);
                    recv_mrs.push(None);
                }
            }
        }
        let peers_mr = PeersSendRecvMR {
            cm_ids: &self.cm_ids,
            my_index: self.my_index,
            num_peers: self.num_peers,
            send_mrs,
            recv_mrs
        };
        Ok(peers_mr)
    }

    pub fn peers(&self) -> usize { self.num_peers }

    pub fn index(&self) -> usize { self.my_index }
}

pub struct PeersSendRecvMR<'cm, T> {
    cm_ids: &'cm Vec<Option<CmId>>,
    my_index: usize,
    num_peers: usize,
    send_mrs: Vec<Option<MemoryRegion<T>>>,
    recv_mrs: Vec<Option<MemoryRegion<T>>>
}

impl<'cm, T: Sized + Copy> PeersSendRecvMR<'cm, T> {
    pub fn post_send<R>(&self, index: usize, range: R, context: u64, flags: SendFlags) -> Result<(), CommunicatorError>
    where
        R: SliceIndex<[T], Output = [T]>
    {
        let send_mr = self.send_mrs.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        Ok(cmid.post_send(send_mr, range, context, flags)?)
    }

    pub unsafe fn post_recv<R>(&mut self, index: usize, range: R, context: u64) -> Result<(), CommunicatorError>
    where
        R: SliceIndex<[T], Output = [T]>
    {
        let recv_mr = self.recv_mrs.get_mut(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_mut()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;

        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;

        Ok(cmid.post_recv(recv_mr, range, context)?)
    }

    pub fn get_send_comp(&self, index: usize) -> Result<WorkCompletion, CommunicatorError> {
        let cmid = self.cm_ids.get(index)
            .ok_or(CommunicatorError::InvalidWorkerIndex)?
            .as_ref()
            .ok_or(CommunicatorError::InvalidWorkerIndex)?;
        
        Ok(cmid.get_send_comp()?)
    }

    pub fn get_recv_comp(&self, index: usize) -> Result<WorkCompletion, CommunicatorError> {
        let cmid = self.cm_ids.get(index)
        .ok_or(CommunicatorError::InvalidWorkerIndex)?
        .as_ref()
        .ok_or(CommunicatorError::InvalidWorkerIndex)?;
    
        Ok(cmid.get_recv_comp()?)
    }

    pub fn peers(&self) -> usize { self.num_peers }

    pub fn index(&self) -> usize { self.my_index }
}


pub trait SendRecvPoll {
    fn recv_poll_blocking(&self) -> Result<(), LibKoalaError>;
    fn recv_poll_nonblocking(&self) -> Result<bool, LibKoalaError>;
    fn send_poll_blocking(&self) -> Result<(), LibKoalaError>;
    fn send_poll_nonblocking(&self) -> Result<bool, LibKoalaError>;
}

impl SendRecvPoll for CmId {
    fn recv_poll_blocking(&self) -> Result<(), LibKoalaError> {
        let cqe = self.get_recv_comp()?;
        assert_eq!(cqe.status, WcStatus::Success);
        Ok(())
    }

    fn recv_poll_nonblocking(&self) -> Result<bool, LibKoalaError> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.qp().recv_cq;
        cq.poll_cq(&mut wc)?;
        if wc.len() == 1 {
            let cqe = wc.pop().unwrap();
            assert_eq!(cqe.status, WcStatus::Success);
            Ok(true)
        }
        else {
            Ok(false)
        }
    }

    fn send_poll_blocking(&self) -> Result<(), LibKoalaError> {
        let cqe = self.get_send_comp()?;
        assert_eq!(cqe.status, WcStatus::Success);
        Ok(())
    }

    fn send_poll_nonblocking(&self) -> Result<bool, LibKoalaError> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.qp().send_cq;
        cq.poll_cq(&mut wc)?;
        if wc.len() == 1 {
            let cqe = wc.pop().unwrap();
            assert_eq!(cqe.status, WcStatus::Success);
            Ok(true)
        }
        else {
            Ok(false)
        }
    }
}