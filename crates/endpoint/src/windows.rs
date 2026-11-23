//! Windows: a named pipe, `\\.\pipe\kibann-<user SID>-<hash of the path>`,
//! that only this user can open (its security descriptor, then the client's
//! and the server's process owner, checked both ways), and a `LockFileEx`
//! lock on `<path>.lock`.
//!
//! Pipe handles are overlapped so a blocked read can be cut short: each
//! read or write waits on its own completion and on the connection's
//! hang-up event, and cancels itself when that is set.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_LOCK_VIOLATION,
    ERROR_NO_DATA, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, LOCKFILE_EXCLUSIVE_LOCK,
    LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, UnlockFileEx, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};

use super::Conn;

/// How much each direction buffers.
const BUFFER: u32 = 64 * 1024;
/// How long a client waits for a free instance when all are taken.
const BUSY_WAIT: Duration = Duration::from_secs(2);

/// Closed when dropped.
struct Handle(HANDLE);

// SAFETY: a kernel handle may be used from any thread.
unsafe impl Send for Handle {}
// SAFETY: as above; every use here is a thread-safe system call.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened and nothing else closes.
        unsafe { CloseHandle(self.0) };
    }
}

/// A manual-reset event, not set.
fn event() -> io::Result<Handle> {
    // SAFETY: no name and default security; a null result is checked.
    let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if h.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(h))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn last_error() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetLastError() }
}

fn os_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

/// The SID (`S-1-5-21-…`) of the user `process` runs as.
fn user_of(process: HANDLE) -> Option<String> {
    let mut token = std::ptr::null_mut();
    // SAFETY: a valid process handle; `token` is written on success.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = Handle(token);
    let mut len = 0u32;
    // A size query: fails with the size needed.
    // SAFETY: a null buffer of length 0, and `len` to write to.
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    if len == 0 {
        return None;
    }
    // u64s to keep the TOKEN_USER aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `len` bytes.
    let ok =
        unsafe { GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
    if ok == 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID lies
    // within it.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = std::ptr::null_mut();
    // SAFETY: a valid SID; the string is freed below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return None;
    }
    // SAFETY: a NUL-terminated string from the call above, freed once.
    unsafe {
        let len = (0..).take_while(|&k| *text.add(k) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Some(s)
    }
}

fn me() -> io::Result<String> {
    // SAFETY: the pseudo-handle needs no closing.
    user_of(unsafe { GetCurrentProcess() }).ok_or_else(io::Error::last_os_error)
}

/// Whether process `pid` runs as `user`.
fn runs_as(pid: u32, user: &str) -> bool {
    // SAFETY: plain arguments; a null result is checked.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return false;
    }
    let h = Handle(h);
    user_of(h.0).as_deref() == Some(user)
}

/// The pipe for `path`. Pipe names are one namespace for every user, so
/// the user's SID is in it; the path's hash keeps `KIBANN_SOCKET`s apart.
fn pipe_name(path: &Path, user: &str) -> String {
    // FNV-1a: stable across builds, unlike std's hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().to_lowercase().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!(r"\\.\pipe\kibann-{user}-{h:016x}")
}

/// Full access for `user` alone (a protected DACL: nothing inherited).
fn sddl(user: &str) -> String {
    format!("D:P(A;;GA;;;{user})")
}

/// A security descriptor, freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

// SAFETY: read-only after it's made.
unsafe impl Send for Descriptor {}
// SAFETY: as above.
unsafe impl Sync for Descriptor {}

impl Descriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let s = wide(sddl);
        let mut sd = std::ptr::null_mut();
        // SAFETY: a NUL-terminated string; `sd` is written on success.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                s.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sd))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion above, freed once.
        unsafe { LocalFree(self.0) };
    }
}

/// One end of a connection, shared by its reader, writer and hang-up.
struct Pipe {
    handle: Handle,
    hangup: Handle,
}

impl Pipe {
    fn new(handle: Handle) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            handle,
            hangup: event()?,
        }))
    }

    fn hung_up(&self) -> bool {
        // SAFETY: a valid event; a zero wait only looks.
        unsafe { WaitForSingleObject(self.hangup.0, 0) == WAIT_OBJECT_0 }
    }

    /// Run `start` (a ReadFile or WriteFile on this pipe) and wait for it,
    /// or for the hang-up, which cancels it.
    fn io(&self, done: &Handle, start: impl FnOnce(*mut OVERLAPPED) -> i32) -> io::Result<usize> {
        if self.hung_up() {
            return Err(os_error(ERROR_OPERATION_ABORTED));
        }
        // SAFETY: a plain C struct; zero is a valid value.
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = done.0;
        if start(&mut ov) == 0 {
            let e = last_error();
            if e != ERROR_IO_PENDING {
                return Err(os_error(e));
            }
            let both = [done.0, self.hangup.0];
            // SAFETY: two valid events.
            let r = unsafe { WaitForMultipleObjects(2, both.as_ptr(), 0, INFINITE) };
            if r == WAIT_OBJECT_0 + 1 {
                // SAFETY: cancels this operation only; it still completes
                // (as aborted), which the wait below sees.
                unsafe { CancelIoEx(self.handle.0, &ov) };
            }
        }
        let mut n = 0u32;
        // SAFETY: `ov` is the operation started above; waiting for it means
        // it's done with `ov` and the buffer before either goes away.
        if unsafe { GetOverlappedResult(self.handle.0, &ov, &mut n, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }
}

/// A reader or writer: the pipe, and an event of its own for completions.
struct Half {
    pipe: Arc<Pipe>,
    done: Handle,
}

impl Read for Half {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let h = self.pipe.handle.0;
        let r = self.pipe.io(&self.done, |ov| {
            // SAFETY: `buf` outlives the operation (`io` waits for it).
            unsafe { ReadFile(h, buf.as_mut_ptr(), len, std::ptr::null_mut(), ov) }
        });
        match r {
            // The other end went, or this one hung up: the end.
            Err(e)
                if [
                    ERROR_BROKEN_PIPE,
                    ERROR_PIPE_NOT_CONNECTED,
                    ERROR_NO_DATA,
                    ERROR_OPERATION_ABORTED,
                ]
                .contains(&(e.raw_os_error().unwrap_or(0) as u32)) =>
            {
                Ok(0)
            }
            r => r,
        }
    }
}

impl Write for Half {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let h = self.pipe.handle.0;
        self.pipe
            .io(&self.done, |ov| {
                // SAFETY: as in `read`.
                unsafe { WriteFile(h, buf.as_ptr(), len, std::ptr::null_mut(), ov) }
            })
            .map_err(|e| match e.raw_os_error().map(|c| c as u32) {
                Some(ERROR_OPERATION_ABORTED | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED) => {
                    io::Error::from(io::ErrorKind::BrokenPipe)
                }
                _ => e,
            })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn conn(pipe: Arc<Pipe>) -> io::Result<Conn> {
    let half = |pipe: &Arc<Pipe>| -> io::Result<Half> {
        Ok(Half {
            pipe: pipe.clone(),
            done: event()?,
        })
    };
    let (reader, writer) = (half(&pipe)?, half(&pipe)?);
    Ok(Conn {
        reader: Box::new(reader),
        writer: Box::new(writer),
        // The handle closes when the reader and writer are dropped.
        hangup: Box::new(move || {
            // SAFETY: a valid event.
            unsafe { SetEvent(pipe.hangup.0) };
        }),
    })
}

/// The directory for the lock and the log. `%TEMP%` is the user's own.
pub fn prepare(path: &Path) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("socket path has no directory"))?;
    std::fs::create_dir_all(dir)
}

pub fn connect(path: &Path) -> io::Result<Conn> {
    let user = me()?;
    let name = wide(&pipe_name(path, &user));
    let deadline = std::time::Instant::now() + BUSY_WAIT;
    let h = loop {
        // SAFETY: a NUL-terminated name; an invalid handle is checked. The
        // server may only identify this client, not act as it.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            break Handle(h);
        }
        let e = last_error();
        // Every instance is taken: the daemon makes another as it accepts
        // (unless it's ending, with clients still connected).
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ms = left.as_millis() as u32;
        // SAFETY: a NUL-terminated name.
        if e == ERROR_PIPE_BUSY && ms > 0 && unsafe { WaitNamedPipeW(name.as_ptr(), ms) } != 0 {
            continue;
        }
        return Err(os_error(e));
    };
    // Another user could have made a pipe of this name first.
    let mut pid = 0u32;
    // SAFETY: a valid pipe handle and an out-parameter.
    if unsafe { GetNamedPipeServerProcessId(h.0, &mut pid) } == 0 || !runs_as(pid, &user) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the terminal service's pipe belongs to another user",
        ));
    }
    conn(Pipe::new(h)?)
}

/// An exclusive `LockFileEx` lock, held while the file stays open.
pub struct Lock(#[allow(dead_code)] File);

fn try_lock(f: &File) -> io::Result<bool> {
    // SAFETY: a plain C struct; zero (offset 0) is what's wanted.
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: a valid file handle; with FAIL_IMMEDIATELY it doesn't wait.
    let ok = unsafe {
        LockFileEx(
            f.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut ov,
        )
    };
    if ok != 0 {
        return Ok(true);
    }
    match last_error() {
        ERROR_LOCK_VIOLATION | ERROR_IO_PENDING => Ok(false),
        e => Err(os_error(e)),
    }
}

impl Lock {
    pub fn take(path: &Path) -> io::Result<Self> {
        let f = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        if !try_lock(&f)? {
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
    match try_lock(&f) {
        Ok(true) => {
            // SAFETY: a plain C struct, offset 0 as locked.
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            // SAFETY: the range locked just now.
            unsafe { UnlockFileEx(f.as_raw_handle(), 0, 1, 0, &mut ov) };
            false
        }
        Ok(false) => true,
        Err(_) => false,
    }
}

/// The instance waiting for the next client.
struct Waiting {
    handle: Handle,
    /// Boxed: the system writes to it until the connect completes.
    ov: Box<OVERLAPPED>,
    done: Handle,
    /// The connect is done (or was before it began): nothing pending.
    connected: bool,
}

// SAFETY: the OVERLAPPED is only touched under the listener's lock and by
// the system.
unsafe impl Send for Waiting {}

impl Waiting {
    /// The connected pipe; the rest is freed.
    fn into_handle(self) -> Handle {
        debug_assert!(self.connected);
        let me = std::mem::ManuallyDrop::new(self);
        // SAFETY: each field is moved out once and `me` is never used or
        // dropped again; the connect is complete, so `ov` is free to go.
        unsafe {
            drop(std::ptr::read(&me.ov));
            drop(std::ptr::read(&me.done));
            std::ptr::read(&me.handle)
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if !self.connected {
            let mut n = 0;
            // SAFETY: cancel the pending connect and wait for it to finish
            // with `ov` before it's freed.
            unsafe {
                CancelIoEx(self.handle.0, &*self.ov);
                GetOverlappedResult(self.handle.0, &*self.ov, &mut n, 1);
            }
        }
    }
}

pub struct Listener {
    name: Vec<u16>,
    user: String,
    descriptor: Descriptor,
    waiting: Mutex<Option<Waiting>>,
}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        let user = me()?;
        let l = Self {
            name: wide(&pipe_name(path, &user)),
            descriptor: Descriptor::new(&sddl(&user))?,
            user,
            waiting: Mutex::new(None),
        };
        // The first instance: fails if any other process has the name.
        let first = l.instance(true).map_err(|e| {
            if e.kind() == io::ErrorKind::PermissionDenied {
                io::Error::new(io::ErrorKind::AddrInUse, "the pipe is taken")
            } else {
                e
            }
        })?;
        *l.waiting.lock().unwrap() = Some(first);
        Ok(l)
    }

    /// A new instance of the pipe, waiting for a client.
    fn instance(&self, first: bool) -> io::Result<Waiting> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: 0,
        };
        let open = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: a NUL-terminated name and valid security attributes; an
        // invalid handle is checked.
        let h = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &sa,
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let handle = Handle(h);
        let done = event()?;
        // SAFETY: a plain C struct; zero is a valid value.
        let mut ov: Box<OVERLAPPED> = Box::new(unsafe { std::mem::zeroed() });
        ov.hEvent = done.0;
        // SAFETY: `ov` is boxed and kept with the handle until the connect
        // completes or is cancelled (`Waiting`'s drop).
        let ok = unsafe { ConnectNamedPipe(handle.0, &mut *ov) };
        let connected = ok != 0
            || match last_error() {
                ERROR_PIPE_CONNECTED => true,
                ERROR_IO_PENDING => false,
                e => return Err(os_error(e)),
            };
        Ok(Waiting {
            handle,
            ov,
            done,
            connected,
        })
    }

    pub fn accept(&self, timeout: Duration) -> io::Result<Option<Conn>> {
        let mut slot = self.waiting.lock().unwrap();
        let w = slot.as_mut().expect("an instance is always waiting");
        if !w.connected {
            let ms = timeout.as_millis().min(u128::from(INFINITE - 1)) as u32;
            // SAFETY: a valid event.
            if unsafe { WaitForSingleObject(w.done.0, ms) } == WAIT_TIMEOUT {
                return Ok(None);
            }
            let mut n = 0;
            // SAFETY: the connect started with `w.ov`, now complete.
            let ok = unsafe { GetOverlappedResult(w.handle.0, &*w.ov, &mut n, 0) } != 0;
            if !ok && last_error() == ERROR_IO_INCOMPLETE {
                return Ok(None);
            }
            // Done, one way or the other: nothing to cancel any more.
            w.connected = true;
            if !ok {
                // The client left already: start over.
                *slot = Some(self.instance(false)?);
                return Ok(None);
            }
        }
        // There must always be an instance waiting, or a client finds none
        // and takes the service for gone.
        let next = self.instance(false)?;
        let w = slot.replace(next).expect("an instance is always waiting");
        let mut pid = 0u32;
        // SAFETY: a connected pipe handle and an out-parameter.
        let ours = unsafe { GetNamedPipeClientProcessId(w.handle.0, &mut pid) } != 0
            && runs_as(pid, &self.user);
        if !ours {
            return Ok(None);
        }
        conn(Pipe::new(w.into_handle())?).map(Some)
    }
}

/// Nothing is left on disk: the pipe goes with its last handle.
pub fn remove(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipe_is_this_users_alone() {
        let user = "S-1-5-21-1-2-3-1001";
        assert_eq!(sddl(user), "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)");
        let a = pipe_name(
            Path::new(r"C:\Users\a\AppData\Local\Temp\kibann\daemon.sock"),
            user,
        );
        assert!(a.starts_with(r"\\.\pipe\kibann-S-1-5-21-1-2-3-1001-"));
        assert_eq!(
            a,
            pipe_name(
                Path::new(r"c:\users\A\AppData\Local\Temp\kibann\daemon.sock"),
                user
            )
        );
        assert_ne!(a, pipe_name(Path::new(r"C:\x\daemon.sock"), user));
    }

    #[test]
    fn a_client_and_the_daemon_talk() {
        let dir = std::env::temp_dir().join(format!("kibann-endpoint-{}", std::process::id()));
        let path = dir.join("test.sock");
        let l = crate::Listener::bind(&path).unwrap();
        assert!(crate::lock_held(&path));
        assert_eq!(
            crate::Listener::bind(&path).err().map(|e| e.kind()),
            Some(io::ErrorKind::AddrInUse)
        );
        assert!(l.accept(Duration::from_millis(10)).unwrap().is_none());
        let client = std::thread::spawn({
            let path = path.clone();
            move || {
                let mut c = crate::connect(&path).unwrap();
                c.writer.write_all(b"ping").unwrap();
                let mut got = [0; 4];
                c.reader.read_exact(&mut got).unwrap();
                assert_eq!(&got, b"pong");
                // A blocked read ends at a hang-up.
                let hangup = c.hangup;
                let mut reader = c.reader;
                let t = std::thread::spawn(move || reader.read(&mut [0; 8]).unwrap());
                std::thread::sleep(Duration::from_millis(50));
                hangup();
                assert_eq!(t.join().unwrap(), 0);
            }
        });
        let mut s = loop {
            if let Some(c) = l.accept(Duration::from_secs(5)).unwrap() {
                break c;
            }
        };
        let mut got = [0; 4];
        s.reader.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping");
        s.writer.write_all(b"pong").unwrap();
        client.join().unwrap();
        // The client's handles are gone: the end.
        assert_eq!(s.reader.read(&mut [0; 8]).unwrap(), 0);
        drop(s);
        let lock = l.close();
        assert!(crate::connect(&path).is_err());
        drop(lock);
        assert!(!crate::lock_held(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
