//! Windows (named pipes) is still to come; see `plans/windows.md`.

use std::io;
use std::path::Path;
use std::time::Duration;

use super::Conn;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the terminal service doesn't run on this platform yet",
    )
}

pub fn prepare(_path: &Path) -> io::Result<()> {
    Err(unsupported())
}

pub fn connect(_path: &Path) -> io::Result<Conn> {
    Err(unsupported())
}

pub struct Lock;

impl Lock {
    pub fn take(_path: &Path) -> io::Result<Self> {
        Err(unsupported())
    }
}

pub fn lock_held(_path: &Path) -> bool {
    false
}

pub struct Listener;

impl Listener {
    pub fn bind(_path: &Path) -> io::Result<Self> {
        Err(unsupported())
    }

    pub fn accept(&self, _timeout: Duration) -> io::Result<Option<Conn>> {
        Err(unsupported())
    }
}

pub fn remove(_path: &Path) {}
