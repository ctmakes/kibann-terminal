//! An in-memory byte pipe, for running the daemon core inside the app (and
//! in tests) with the same code path as a socket.

use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};

pub struct PipeWriter(Sender<Vec<u8>>);

pub struct PipeReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

/// A one-way pipe. Reading returns end of file once the writer is dropped
/// and everything written has been read.
pub fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = channel();
    (
        PipeWriter(tx),
        PipeReader {
            rx,
            buf: Vec::new(),
            pos: 0,
        },
    )
}

impl Write for PipeWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0
            .send(data.to_vec())
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buf = chunk;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_arrive_in_order_then_eof() {
        let (mut w, mut r) = pipe();
        w.write_all(b"hello ").unwrap();
        w.write_all(b"world").unwrap();
        drop(w);
        let mut s = String::new();
        r.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");
    }
}
