use std::collections::VecDeque;
use std::io;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::slice;

use bytes::{BufMut, Bytes, BytesMut};

/// A nonblocking version of a std::io::Lines iterator.
pub trait NonBlockingLineReader {
    fn eof(&self) -> bool;

    /// On success, returns a new line from the reader or `None` if there's no new line currently.
    /// On error, returns the underlying io::Error during the read.
    fn next_line(&mut self) -> io::Result<Option<Bytes>>;
}

pub struct LineReader<R> {
    reader: R,
    lines: VecDeque<Bytes>,
    buffer: BytesMut,
    eof: bool,
}

impl<R: Read + AsRawFd> LineReader<R> {
    pub fn new(reader: R) -> Self {
        tokio_anyfd::set_nonblocking(reader.as_raw_fd()).unwrap();
        Self {
            reader,
            lines: VecDeque::new(),
            buffer: BytesMut::with_capacity(1024),
            eof: false,
        }
    }
}

impl<R: Read> NonBlockingLineReader for LineReader<R> {
    #[inline]
    fn eof(&self) -> bool {
        self.eof
    }

    fn next_line(&mut self) -> io::Result<Option<Bytes>> {
        if !self.lines.is_empty() {
            return Ok(self.lines.pop_front());
        }

        if self.eof {
            return Ok(None);
        }

        if self.buffer.capacity() == self.buffer.len() {
            // fill the buffer with more space
            self.buffer.reserve(self.buffer.len());
        }

        let buffer = unsafe {
            slice::from_raw_parts_mut(
                self.buffer.chunk_mut().as_mut_ptr(),
                self.buffer.chunk_mut().len(),
            )
        };
        match self.reader.read(buffer) {
            Ok(0) => {
                // EOF
                self.eof = true;
                Ok(None)
            }
            Ok(nbytes) => {
                let mut old_len = -(self.buffer.len() as isize);
                unsafe { self.buffer.advance_mut(nbytes) };
                for (pos, _) in buffer
                    .iter()
                    .take(nbytes)
                    .enumerate()
                    .filter(|i| *i.1 == b'\n')
                {
                    let mut new_line = self
                        .buffer
                        .split_to((pos as isize - old_len) as usize + 1)
                        .freeze();
                    new_line.truncate(new_line.len() - 1);
                    self.lines.push_back(new_line);
                    old_len = pos as isize + 1;
                }
                Ok(self.lines.pop_front())
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

// #[test]
// fn test_line_reader1() {
//     use bytes::Buf;
//     let buf = Bytes::copy_from_slice(b"hello\n world\n\n!\n");
//     let mut line_reader = LineReader::new(buf.reader());
//     while let Some(line) = line_reader.next_line().unwrap() {
//         println!("{}", std::str::from_utf8(&line).unwrap());
//     }
// }
