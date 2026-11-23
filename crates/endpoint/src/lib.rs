//! Where the daemon and its clients meet, named by a path: a Unix socket
//! there (a named pipe on Windows, to come), and beside it the lock the
//! daemon holds for as long as its process runs (`<path>.lock`).
//!
//! Nothing above this module knows which: a connection is a reader, a
//! writer and a way to hang up, on both sides.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One connection, either side.
pub struct Conn {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    /// Closes the connection both ways (the reader may be blocked in
    /// another thread).
    pub hangup: Box<dyn Fn() + Send + Sync>,
}

/// The lock a daemon holds for as long as it runs.
fn lock_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".lock");
    p.into()
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as os;

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
use unsupported as os;

/// Connect to the daemon at `path`.
pub fn connect(path: &Path) -> io::Result<Conn> {
    os::connect(path)
}

/// Whether a daemon for `path` is running, or still ending: it holds the
/// lock until its process is gone.
pub fn lock_held(path: &Path) -> bool {
    os::lock_held(&lock_path(path))
}

/// The daemon's end: holds the lock, and takes connections from this user
/// only (anyone who can connect can type into every shell).
pub struct Listener {
    inner: os::Listener,
    lock: Lock,
    path: PathBuf,
}

/// Held for as long as the daemon runs; see [`Listener::close`].
pub struct Lock {
    _inner: os::Lock,
}

impl Listener {
    /// Take `path`. Fails with `AddrInUse` if another daemon has it.
    pub fn bind(path: &Path) -> io::Result<Self> {
        os::prepare(path)?;
        let lock = Lock {
            _inner: os::Lock::take(&lock_path(path))?,
        };
        // Holding the lock means whatever is left at `path` is stale.
        let inner = os::Listener::bind(path)?;
        Ok(Self {
            inner,
            lock,
            path: path.to_owned(),
        })
    }

    /// Wait up to `timeout` for the next client. `None` if none came.
    pub fn accept(&self, timeout: Duration) -> io::Result<Option<Conn>> {
        self.inner.accept(timeout)
    }

    /// Stop taking connections (new clients find nobody there), keeping
    /// the lock: the daemon holds it until it's done ending.
    pub fn close(self) -> Lock {
        drop(self.inner);
        os::remove(&self.path);
        self.lock
    }
}
