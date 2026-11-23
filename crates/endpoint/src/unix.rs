use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use super::Conn;

fn conn(stream: UnixStream) -> io::Result<Conn> {
    let hangup = stream.try_clone()?;
    Ok(Conn {
        reader: Box::new(stream.try_clone()?),
        writer: Box::new(stream),
        hangup: Box::new(move || {
            let _ = hangup.shutdown(std::net::Shutdown::Both);
        }),
    })
}

/// The directory, private to this user.
pub fn prepare(path: &Path) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("socket path has no directory"))?;
    proto::paths::prepare_dir(dir)
}

pub fn connect(path: &Path) -> io::Result<Conn> {
    conn(UnixStream::connect(path)?)
}

/// An exclusive `flock`, held while the file stays open.
pub struct Lock(#[allow(dead_code)] File);

impl Lock {
    pub fn take(path: &Path) -> io::Result<Self> {
        let f = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        // SAFETY: a valid open file descriptor.
        let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if r != 0 {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another daemon is running",
            ));
        }
        Ok(Self(f))
    }
}

pub fn lock_held(path: &Path) -> bool {
    let Ok(f) = File::options().read(true).write(true).open(path) else {
        return false;
    };
    // SAFETY: a valid open file descriptor; the lock goes with the file.
    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) != 0 }
}

pub struct Listener(UnixListener);

impl Listener {
    /// Made as `<path>.n` and renamed into place once it listens, private:
    /// a client never finds a socket at `path` that refuses it (binding
    /// creates the file before listening starts). In place when the longer
    /// name doesn't fit in a socket address.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let mut new = path.as_os_str().to_owned();
        new.push(".n");
        let new = std::path::PathBuf::from(new);
        let _ = std::fs::remove_file(&new);
        let (listener, made) = match UnixListener::bind(&new) {
            Ok(l) => (l, new.as_path()),
            Err(_) => {
                let _ = std::fs::remove_file(path);
                (UnixListener::bind(path)?, path)
            }
        };
        std::fs::set_permissions(made, std::fs::Permissions::from_mode(0o600))?;
        if made != path {
            std::fs::rename(made, path)?;
        }
        listener.set_nonblocking(true)?;
        Ok(Self(listener))
    }

    pub fn accept(&self, timeout: Duration) -> io::Result<Option<Conn>> {
        let mut fds = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd.
        let r = unsafe { libc::poll(&mut fds, 1, ms) };
        if r < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted {
                Ok(None)
            } else {
                Err(e)
            };
        }
        match self.0.accept() {
            Ok((stream, _)) => {
                // It inherits the listener's non-blocking mode on some
                // systems.
                stream.set_nonblocking(false)?;
                if peer_uid(&stream) != Some(euid()) {
                    return Ok(None);
                }
                conn(stream).map(Some)
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

/// The user id of the process at the other end of `s`.
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let fd = s.as_raw_fd();
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` are valid for SO_PEERCRED.
        let r = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast(),
                &mut len,
            )
        };
        (r == 0).then_some(cred.uid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: plain out-parameters.
        let r = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        (r == 0).then_some(uid)
    }
}
