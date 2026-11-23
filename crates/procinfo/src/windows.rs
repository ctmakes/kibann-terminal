//! Windows: Toolhelp32 for names and parents, `NtQueryInformationProcess`
//! for command lines.

use std::path::PathBuf;

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

/// Closed when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened and nothing else closes.
        unsafe { CloseHandle(self.0) };
    }
}

fn open(pid: i32) -> Option<Handle> {
    let pid = u32::try_from(pid).ok()?;
    // SAFETY: plain arguments; a null result is checked.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    (!h.is_null()).then_some(Handle(h))
}

/// One process in a snapshot.
struct Entry {
    pid: u32,
    parent: u32,
    exe: String,
}

/// Every process now.
fn processes() -> Vec<Entry> {
    // SAFETY: plain arguments; an invalid handle is checked.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let snap = Handle(snap);
    // SAFETY: a plain C struct; zero is a valid value.
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut out = Vec::new();
    // SAFETY: `e` is valid and its size set, as the API asks.
    let mut ok = unsafe { Process32FirstW(snap.0, &mut e) } != 0;
    while ok {
        let len = e
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(e.szExeFile.len());
        out.push(Entry {
            pid: e.th32ProcessID,
            parent: e.th32ParentProcessID,
            exe: String::from_utf16_lossy(&e.szExeFile[..len]),
        });
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap.0, &mut e) } != 0;
    }
    out
}

/// Not known on Windows without reading the process's memory; shells
/// report it themselves (OSC 7, OSC 9;9).
pub fn cwd(_pid: i32) -> Option<PathBuf> {
    None
}

/// The program's file name without `.exe` (`pwsh`, `vim`).
pub fn name(pid: i32) -> Option<String> {
    let pid = u32::try_from(pid).ok()?;
    let exe = processes().into_iter().find(|e| e.pid == pid)?.exe;
    Some(match exe.len().checked_sub(4) {
        Some(n) if exe[n..].eq_ignore_ascii_case(".exe") => exe[..n].to_owned(),
        _ => exe,
    })
}

/// The process's command line, split as the C runtime does.
pub fn argv(pid: i32) -> Option<Vec<String>> {
    let h = open(pid)?;
    let mut len = 0u32;
    // A size query: fails with the size needed.
    // SAFETY: a null buffer of length 0, and `len` to write to.
    unsafe {
        NtQueryInformationProcess(
            h.0,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut len,
        )
    };
    if len == 0 {
        return None;
    }
    // A UNICODE_STRING, its text right after it. u64s to keep it aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `len` bytes.
    let status = unsafe {
        NtQueryInformationProcess(
            h.0,
            ProcessCommandLineInformation,
            buf.as_mut_ptr().cast(),
            len,
            &mut len,
        )
    };
    if status < 0 {
        return None;
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum: u16,
        buffer: *const u16,
    }
    // SAFETY: on success the buffer starts with a UNICODE_STRING whose text
    // lies within the buffer.
    let line: Vec<u16> = unsafe {
        let s = &*buf.as_ptr().cast::<UnicodeString>();
        if s.buffer.is_null() {
            return None;
        }
        std::slice::from_raw_parts(s.buffer, usize::from(s.length) / 2).to_vec()
    };
    split(&line)
}

/// `line` split into arguments by `CommandLineToArgvW`.
fn split(line: &[u16]) -> Option<Vec<String>> {
    let mut z = line.to_vec();
    z.push(0);
    let mut n = 0i32;
    // SAFETY: `z` is NUL-terminated; the result is freed below.
    let list = unsafe { CommandLineToArgvW(z.as_ptr(), &mut n) };
    if list.is_null() {
        return None;
    }
    let args = (0..usize::try_from(n).unwrap_or(0))
        .map(|i| {
            // SAFETY: `list` has `n` NUL-terminated strings.
            unsafe {
                let p = *list.add(i);
                let len = (0..).take_while(|&k| *p.add(k) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
            }
        })
        .collect();
    // SAFETY: allocated by CommandLineToArgvW, freed once.
    unsafe { LocalFree(list.cast()) };
    Some(args)
}

/// When the process started (100 ns since 1601), if it can be asked.
fn started(pid: u32) -> Option<u64> {
    let h = open(i32::try_from(pid).ok()?)?;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four valid FILETIMEs to write.
    let ok = unsafe { GetProcessTimes(h.0, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

/// The newest process started under `shell` (its children, theirs…): what
/// the user ran last, since a console has no foreground process group. A
/// pid can be reused, so a "child" that started before its parent isn't one.
pub fn newest_descendant(shell: i32) -> Option<i32> {
    let shell = u32::try_from(shell).ok()?;
    let all = processes();
    let mut found: Vec<(u32, u64)> = Vec::new();
    let mut parents = vec![(shell, started(shell)?)];
    while let Some((parent, since)) = parents.pop() {
        for e in all.iter().filter(|e| e.parent == parent && e.pid != parent) {
            if let Some(t) = started(e.pid).filter(|t| *t >= since) {
                found.push((e.pid, t));
                parents.push((e.pid, t));
            }
        }
    }
    let (pid, _) = found.into_iter().max_by_key(|(_, t)| *t)?;
    i32::try_from(pid).ok()
}
