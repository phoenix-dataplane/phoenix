use std::mem;
use std::ops::{Range, AddAssign};
use std::collections::VecDeque;

use libkoala::verbs::SendFlags;

use super::communicator::{Communicator, SendRecvPoll};
use super::AllReduceError;

#[inline]
fn ceil_div(x: usize, y: usize) -> usize {
    x / y + (x % y != 0) as usize
}

// chunk_bytes: num of bytes per chunk
fn split_into_buffer(
    num_elements: usize, 
    sizeof_element: usize, 
    num_peers: usize, 
    chunk_bytes: Option<usize>) -> Result<Vec<Range<usize>>, AllReduceError> 
{
    let elements_per_chunk;
    let num_chunks;
    if num_elements < num_peers {
        return Err(AllReduceError::InvalidArgument("Input vector is too short for RingAllreduce"));
    }
    if let Some(chunk_bytes) = chunk_bytes {
        if chunk_bytes % sizeof_element != 0 {
            return Err(AllReduceError::InvalidArgument("Selected chunk_bytes is not divisible by `size_of::<T>()`"));
        }

        elements_per_chunk = chunk_bytes / sizeof_element;
        num_chunks = ceil_div(num_elements, elements_per_chunk);

        if num_chunks < 2 * num_peers {
            return Err(AllReduceError::InvalidArgument("Selected chunk_bytes is too large"));
        }
    }
    else {
        num_chunks = 4 * num_peers;
        elements_per_chunk = ceil_div(num_elements, num_chunks);
    }

    let mut ranges = Vec::with_capacity(num_chunks);
    for i in 0..num_chunks {
        let start = elements_per_chunk * i;
        let end = num_elements.min(elements_per_chunk * (i + 1));

        if start < end {
            let range = Range {
                start,
                end
            };
            ranges.push(range);
        }
    }
    Ok(ranges)
}

fn get_workers_chunks(ranges: &Vec<Range<usize>>, num_peers: usize) -> Vec<Vec<usize>> {
    let mut workers_to_chunks = Vec::with_capacity(num_peers);
    let chunks_per_worker = ranges.len() / num_peers;
    let remaining_chunks = ranges.len() % num_peers;

    let mut curr_chunk = 0;
    for index in 0..num_peers {
        let mut allocated_chunks = Vec::with_capacity(chunks_per_worker + 1);
        for _i in 0..chunks_per_worker {
            allocated_chunks.push(curr_chunk);
            curr_chunk += 1;
        }

        if index < remaining_chunks {
            allocated_chunks.push(curr_chunk);
            curr_chunk += 1;
        }
        workers_to_chunks.push(allocated_chunks);
    }
    workers_to_chunks
}

fn reduce_in_place<T: Sized + Clone + AddAssign>(dst: &mut [T], src: &[T], range: &Range<usize>) {
    for (target, source) in dst[range.clone()].iter_mut().zip(src[range.clone()].iter()) {
        *target += source.clone();
    }
}

pub fn allreduce_ring<T: Sized + Copy + AddAssign>(
    src_data: &[T], 
    communicator: &Communicator, 
    chunk_bytes: Option<usize>) -> Result<Vec<T>, AllReduceError>
{
    let num_elements = src_data.len();
    let num_peers = communicator.peers();
    let my_index = communicator.peers();

    // worker index to receive data from
    let from_worker = (my_index - 1 + num_peers) % num_peers;
    let mut recv_mr = communicator.allocate_msgs(from_worker, num_elements)?;
    let recv_cmid = communicator.get_cmid(from_worker)?;
    // worker index to send data to
    let to_worker = (my_index + 1) % num_peers;
    let mut send_mr = communicator.allocate_msgs(to_worker, num_elements)?;
    send_mr.copy_from_slice(src_data);
    
    let send_cmid = communicator.get_cmid(to_worker)?;

    let ranges = split_into_buffer(num_elements, mem::size_of::<T>(), num_peers, chunk_bytes)?;
    let num_chunks = ranges.len();
    let workers_to_chunks = get_workers_chunks(&ranges, num_peers);
    
    let mut to_request_chunks = VecDeque::new();
    // round in ReduceScatter phase 
    for round in 0..num_peers - 1 {
        let index = (my_index - round  - 1 + num_peers) % num_peers;

        for chunk in workers_to_chunks[index].iter().rev() {
            to_request_chunks.push_back(*chunk);
        }
    }

    let mut bootstrap_to_send_chunks = VecDeque::new();
    let mut to_send_chunks_buffer = VecDeque::new();

    for chunk in workers_to_chunks[my_index].iter().rev() {
        bootstrap_to_send_chunks.push_back(*chunk);
    }

    let mut curr_to_recv_chunk = to_request_chunks.front().unwrap().to_owned();
    let mut recv_wqe = 0;
    let mut send_wqe = 0;

    let mut reduce_scatter_finished = false;
    while !reduce_scatter_finished {
        while (to_request_chunks.is_empty() || recv_wqe > 0) && recv_cmid.recv_poll_nonblocking()? {
            // ensure all outstanding recv wr (work request) are completed
            recv_wqe -= 1;
            let range = &ranges[curr_to_recv_chunk];
            reduce_in_place(&mut recv_mr, &send_mr, range);
            
            // workers_to_chunks[to_worker][0] is the last chunk to receive
            if curr_to_recv_chunk == workers_to_chunks[to_worker][0] {
                reduce_scatter_finished = true;
            }

            if !(workers_to_chunks[to_worker][0] <= curr_to_recv_chunk && 
                curr_to_recv_chunk <= *workers_to_chunks[to_worker].last().unwrap()) {
                if bootstrap_to_send_chunks.is_empty() && to_send_chunks_buffer.is_empty() {
                    send_cmid.post_send(&recv_mr, range.clone(), 0, SendFlags::SIGNALED)?;
                    send_wqe += 1;
                }
                else {
                    to_send_chunks_buffer.push_back(curr_to_recv_chunk);
                }
            }
            curr_to_recv_chunk = (curr_to_recv_chunk - 1 + num_chunks) % num_chunks;
        }

        if !to_request_chunks.is_empty() {
            let req_chunk = to_request_chunks.pop_front().unwrap();
            
            let range = &ranges[req_chunk];
            unsafe {
                recv_cmid.post_recv(&mut recv_mr, range.clone(), 0)?;
            }
            recv_wqe += 1;
        }

        if !bootstrap_to_send_chunks.is_empty() {
            let send_chunk = bootstrap_to_send_chunks.pop_front().unwrap();
            let range = &ranges[send_chunk];
            send_cmid.post_send(&send_mr, range.clone(), 0, SendFlags::SIGNALED)?;
            send_wqe += 1;
        }
        else {
            while !to_send_chunks_buffer.is_empty() {
                let send_chunk = to_send_chunks_buffer.pop_front().unwrap();
                let range = &ranges[send_chunk];
                send_cmid.post_send(&recv_mr, range.clone(), 0, SendFlags::SIGNALED)?;
                send_wqe += 1;
            }
        }
        
        while send_cmid.send_poll_nonblocking()? {
            send_wqe -= 1;
        } 
    }
    assert_eq!(recv_wqe, 0);
    while send_wqe != 0 {
        send_cmid.send_poll_blocking()?;
        send_wqe -= 1;
    }

    for round in 0..num_peers {
        let index = (my_index + 1 - round + num_peers) % num_peers;
        for chunk in workers_to_chunks[index].iter().rev() {
            let range = &ranges[*chunk];

            // recv except the first round
            if index != (my_index + 1) % num_peers {
                unsafe {
                    recv_cmid.post_recv(&mut recv_mr, range.clone(), 0)?;
                }
                while !recv_cmid.recv_poll_nonblocking()? {
                    if send_cmid.send_poll_nonblocking()? {
                        send_wqe -= 1;
                    }
                }
            }
            
            // send except the last round
            if index != (my_index + 2) % num_peers {
                send_cmid.post_send(&recv_mr, range.clone(), 0, SendFlags::SIGNALED)?;
                send_wqe += 1;
            }
        }
    }

    while send_wqe != 0 {
        send_cmid.send_poll_blocking()?;
        send_wqe -= 1;
    }

    let agg = recv_mr.to_vec();

    Ok(agg)
}