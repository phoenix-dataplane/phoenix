use std::io;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};

pub struct DevNull;

impl Write for DevNull {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// An adapter for readers whose inputs
/// are written to a "tee"'d writer
pub struct TeeReader<R: Read, W: Write> {
    reader: R,
    writer: W,
}

impl<R: Read + AsRawFd, W: Write> AsRawFd for TeeReader<R, W> {
    fn as_raw_fd(&self) -> RawFd {
        self.reader.as_raw_fd()
    }
}

impl<R: Read, W: Write> TeeReader<R, W> {
    /// Returns a TeeReader which can be used as Read whose
    /// reads delegate bytes read to the provided reader and write to the provided
    /// writer. The write operation must complete before the read completes.
    ///
    /// Errors reported by the write operation will be interpreted as errors for the read
    pub fn new(reader: R, writer: W) -> TeeReader<R, W> {
        TeeReader { reader, writer }
    }
}

impl<R: Read, W: Write> Read for TeeReader<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(buf)?;
        self.writer.write_all(&buf[..n])?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn tee() {
        let mut reader = "It's over 9000!".as_bytes();
        let mut teeout = Vec::new();
        let mut stdout = Vec::new();
        {
            let mut tee = TeeReader::new(&mut reader, &mut teeout);
            let _ = tee.read_to_end(&mut stdout);
        }
        assert_eq!(teeout, stdout);
    }
}
