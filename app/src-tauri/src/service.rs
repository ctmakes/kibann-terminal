//! The terminal service (the daemon): finding it, starting it, and
//! replacing one from another version.
//!
//! An update replaces the app but leaves the running service alone, so its
//! shells survive. Terminal behaviour lives in the service, so an update's
//! terminal changes only take effect once it's replaced. What the app does
//! with the service it finds ([`plan`]):
//! - **Built from the same code:** uses it.
//! - **Another build, same protocol:** replaces it on its own when every
//!   shell is at its prompt (the tabs come back with fresh shells in the
//!   same directories); otherwise keeps using it and asks.
//! - **Another protocol:** this app can't show its sessions. Replaces it on
//!   its own when idle; otherwise asks to restart it or quit.
//!
//! Dev builds replace any other build at once: their sessions are
//! disposable. The service saves the layout itself before it exits.

use std::io;

use client::{Client, ClientEvent, Fit, Hello};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// What [`connect`] found.
pub enum Connected {
    /// A service this app can use as it is.
    Ready(Client),
    /// Another build's, with programs running in it (so it wasn't replaced
    /// on its own). It works; the update's terminal changes wait.
    Older(Client, Option<Vec<String>>),
    /// Another protocol's, with programs running in it: unusable until
    /// it's replaced.
    Unusable(Option<Vec<String>>),
}

/// What to do with a service, from how it [`Fit`]s.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Use,
    /// End it (it saves its layout first) and start this build's.
    Replace,
    /// Use it, and ask whether to replace it.
    UseAndAsk,
    /// Ask whether to replace it or quit.
    Ask,
}

/// `replaced`: this run already replaced one, so whatever runs now is used
/// if it can be (it would only be replaced again).
fn plan(fit: &Fit, dev: bool, idle: bool, restores: bool, replaced: bool) -> Step {
    match fit {
        Fit::Same => Step::Use,
        Fit::OtherBuild if replaced => Step::Use,
        _ if dev || (idle && restores && !replaced) => Step::Replace,
        Fit::OtherBuild => Step::UseAndAsk,
        Fit::OtherProtocol => Step::Ask,
    }
}

/// Connect to the service, starting it if needed, and replace it when
/// [`plan`] says so. `on_event` makes the event handler for each
/// connection. The service is this same executable run with `--daemon`.
#[cfg(unix)]
pub fn connect<F>(on_event: impl Fn() -> F) -> io::Result<Connected>
where
    F: FnMut(ClientEvent<'_>) + Send + 'static,
{
    let dev = cfg!(debug_assertions);
    let socket = proto::paths::socket_path(dev);
    let command = vec![std::env::current_exe()?.into_os_string(), "--daemon".into()];
    let restores = crate::theme::read_settings().0.restore_layout;
    let mut replaced = false;
    loop {
        let client = Client::connect_or_start(&socket, &command, on_event())?;
        let hello = client.hello()?;
        let fit = hello.fit(daemon::BUILD);
        let (idle, Hello { build, busy, .. }) = (hello.idle(), hello);
        match plan(&fit, dev, idle, restores, replaced) {
            Step::Use => return Ok(Connected::Ready(client)),
            Step::UseAndAsk => return Ok(Connected::Older(client, busy)),
            Step::Ask => return Ok(Connected::Unusable(busy)),
            Step::Replace if replaced => {
                return Err(io::Error::other(
                    "the terminal service is still another version's after restarting it",
                ));
            }
            Step::Replace => {
                eprintln!(
                    "kibann: replacing the terminal service of build {build} with {}",
                    daemon::BUILD
                );
                client.stop(&socket)?;
                replaced = true;
            }
        }
    }
}

#[cfg(not(unix))]
pub fn connect<F>(_on_event: impl Fn() -> F) -> io::Result<Connected>
where
    F: FnMut(ClientEvent<'_>) + Send + 'static,
{
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the terminal service doesn't run on this platform yet",
    ))
}

/// Replace the running service, then start the app again on the new one.
fn replace_and_restart(app: &AppHandle) {
    #[cfg(unix)]
    {
        let socket = proto::paths::socket_path(cfg!(debug_assertions));
        if let Ok(old) = Client::connect(&socket, |_| {})
            && let Err(e) = old.stop(&socket)
        {
            eprintln!("kibann: stopping the terminal service: {e}");
        }
    }
    app.restart();
}

/// "Restarting ends what runs in them (claude and vim)".
fn what_ends(busy: &Option<Vec<String>>) -> String {
    match busy {
        Some(b) if !b.is_empty() => format!(
            "Restarting ends what runs in them ({})",
            client::list_programs(b)
        ),
        _ => "Restarting ends the open terminals".to_owned(),
    }
}

fn after_restart() -> &'static str {
    if crate::theme::read_settings().0.restore_layout {
        "your tabs and splits come back with fresh shells"
    } else {
        "the tabs close (restore-layout is off)"
    }
}

/// The service from an earlier version still runs programs in its shells:
/// ask before replacing it, which ends them.
pub fn offer_update_restart(app: &AppHandle, busy: &Option<Vec<String>>) {
    let handle = app.clone();
    app.dialog()
        .message(format!(
            "This update changes how the terminal behaves, but the terminal service \
             from the earlier version is still running your shells.\n\n\
             {}; {}. Until then the earlier behaviour stays. \
             Next time every shell is at its prompt, the restart happens on its own.",
            what_ends(busy),
            after_restart()
        ))
        .title("Restart the terminal service?")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Restart Now".into(),
            "Later".into(),
        ))
        .show(move |restart| {
            if restart {
                replace_and_restart(&handle);
            }
        });
}

/// The running service is from a version this one can't talk to: restart
/// it, or quit and leave it running.
pub fn offer_restart(app: &AppHandle, busy: &Option<Vec<String>>) {
    let handle = app.clone();
    app.dialog()
        .message(format!(
            "The terminal service from another version of Kibann Terminal is running \
             your shells, and this version can't show them.\n\n\
             {}; {}. Quitting leaves them running.",
            what_ends(busy),
            after_restart()
        ))
        .title("Restart the terminal service?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Restart Service".into(),
            "Quit".into(),
        ))
        .show(move |restart| {
            if restart {
                replace_and_restart(&handle);
            } else {
                handle.exit(0);
            }
        });
}

/// The service couldn't be reached or started.
pub fn no_service(app: &AppHandle, e: &io::Error) {
    let handle = app.clone();
    app.dialog()
        .message(format!(
            "Kibann Terminal couldn't start its terminal service: {e}"
        ))
        .title("No terminal service")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCustom("Quit".into()))
        .show(move |_| handle.exit(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_happens_to_the_service_found() {
        use Step::*;
        let p = |fit, dev, idle, restores, replaced| plan(&fit, dev, idle, restores, replaced);
        // Release: the same build is used; another is replaced while idle.
        assert_eq!(p(Fit::Same, false, false, true, false), Use);
        assert_eq!(p(Fit::OtherBuild, false, true, true, false), Replace);
        assert_eq!(p(Fit::OtherProtocol, false, true, true, false), Replace);
        // Busy, or the tabs wouldn't come back: ask.
        assert_eq!(p(Fit::OtherBuild, false, false, true, false), UseAndAsk);
        assert_eq!(p(Fit::OtherBuild, false, true, false, false), UseAndAsk);
        assert_eq!(p(Fit::OtherProtocol, false, false, true, false), Ask);
        // After a replacement, a usable one is used.
        assert_eq!(p(Fit::OtherBuild, false, true, true, true), Use);
        // Dev: any other build is replaced.
        assert_eq!(p(Fit::OtherBuild, true, false, true, false), Replace);
        assert_eq!(p(Fit::OtherProtocol, true, false, false, false), Replace);
        assert_eq!(p(Fit::Same, true, false, true, false), Use);
    }
}
