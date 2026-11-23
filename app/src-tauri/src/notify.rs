//! System notifications: an agent waits or finished, in a pane the user
//! isn't looking at. A click on one brings the window forward and shows the
//! pane (its id is in the notification).
//!
//! macOS talks to User Notifications itself. The notification plugin there
//! goes through the old `NSUserNotification` API and, to report clicks,
//! keeps a thread and a main-thread timer per notification until it leaves
//! Notification Center; each timer asks the notification server for the
//! whole list twice a second and waits for the answer, so notifications the
//! user never clears pile up until the window stops responding. Here clicks
//! arrive through a delegate and nothing polls. Where User Notifications
//! can't be used (a binary outside an app bundle, as with `tauri dev`, or
//! an app it refuses, as an unsigned one may be) the plugin shows them with
//! no click reporting, which needs no timers.

use tauri::{AppHandle, Emitter};

/// Show the pane a clicked notification is about. A pane closed since
/// does nothing.
fn focus(app: &AppHandle, pane: u32) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    crate::tray::show_window(app);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
    let _ = app.emit("focus-pane", pane);
}

/// Through the plugin. Clicks are reported only where [`init`] registered
/// for them.
fn show_plugin(app: &AppHandle, pane: u32, title: String, body: String) {
    use tauri_plugin_notification::NotificationExt;
    let r = app
        .notification()
        .builder()
        .id(i32::try_from(pane).unwrap_or(0))
        .title(title)
        .body(body)
        .show();
    if let Err(e) = r {
        eprintln!("kibann: notification: {e}");
    }
}

/// Get ready to report clicks (at startup).
pub fn init(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    mac::init(app);
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let handle = app.clone();
        let _ = app.notification().on_action(move |done| {
            if done.action_id() != "tap" {
                return;
            }
            if let Some(n) = done.notification()
                && let Ok(pane) = u32::try_from(n.id())
            {
                focus(&handle, pane);
            }
        });
    }
}

/// Tell the user `title` / `body` about `pane`.
pub fn show(app: &AppHandle, pane: u32, title: String, body: String) {
    #[cfg(target_os = "macos")]
    mac::show(app, pane, title, body);
    #[cfg(not(target_os = "macos"))]
    show_plugin(app, pane, title, body);
}

/// The notification's identifier names its pane, so a newer one about the
/// same pane replaces the one before instead of adding to the list.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn identifier(pane: u32) -> String {
    format!("pane-{pane}")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pane_of(identifier: &str) -> Option<u32> {
    identifier.strip_prefix("pane-")?.parse().ok()
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
        UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };
    use tauri::AppHandle;

    struct Ivars {
        app: AppHandle,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and the
        // delegate doesn't implement Drop.
        #[unsafe(super(NSObject))]
        #[name = "KibannNotificationDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            /// With the app in front: show it anyway, since it's about a
            /// pane the user isn't looking at (another tab, say).
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                done.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
            }

            /// A click on the notification (not on Close).
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                done: &DynBlock<dyn Fn()>,
            ) {
                // SAFETY: an immutable string constant of the framework.
                let default = unsafe { UNNotificationDefaultActionIdentifier };
                if &*response.actionIdentifier() == default {
                    let id = response.notification().request().identifier().to_string();
                    if let Some(pane) = super::pane_of(&id) {
                        super::focus(&self.ivars().app, pane);
                    }
                }
                done.call(());
            }
        }
    );

    /// The center's delegate is a weak reference: this keeps it alive.
    struct Kept(#[allow(dead_code)] Retained<Delegate>);
    // SAFETY: the delegate is only read (its AppHandle, which is Send and
    // Sync) and the class isn't main-thread-only.
    unsafe impl Send for Kept {}
    unsafe impl Sync for Kept {}
    static DELEGATE: OnceLock<Kept> = OnceLock::new();

    /// User Notifications refused this app: the plugin shows them from then on.
    static REFUSED: AtomicBool = AtomicBool::new(false);

    /// User Notifications needs an app bundle; without one getting its
    /// center throws.
    fn bundled() -> bool {
        let b = NSBundle::mainBundle();
        b.bundleIdentifier().is_some() && b.bundlePath().to_string().ends_with(".app")
    }

    pub fn init(app: &AppHandle) {
        if !bundled() {
            return;
        }
        let this = Delegate::alloc().set_ivars(Ivars { app: app.clone() });
        // SAFETY: NSObject's init, on a freshly allocated object.
        let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
        UNUserNotificationCenter::currentNotificationCenter()
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let _ = DELEGATE.set(Kept(delegate));
    }

    pub fn show(app: &AppHandle, pane: u32, title: String, body: String) {
        if DELEGATE.get().is_none() || REFUSED.load(Ordering::Relaxed) {
            return super::show_plugin(app, pane, title, body);
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&super::identifier(pane)),
            &content,
            None,
        );
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let app = app.clone();
        // The first time this asks the user; after that it answers at once
        // with what they chose (in System Settings, too).
        let asked = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            if granted.as_bool() {
                UNUserNotificationCenter::currentNotificationCenter()
                    .addNotificationRequest_withCompletionHandler(&request, None);
            } else if !error.is_null() {
                // Not the user's choice: User Notifications won't take this
                // app (unsigned, say).
                // SAFETY: non-null, valid for the call.
                let e = unsafe { &*error };
                eprintln!(
                    "kibann: notifications: {}; showing them without clicks",
                    e.localizedDescription()
                );
                REFUSED.store(true, Ordering::Relaxed);
                super::show_plugin(&app, pane, title.clone(), body.clone());
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &asked,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identifier_names_the_pane() {
        assert_eq!(pane_of(&identifier(7)), Some(7));
        assert_eq!(pane_of(&identifier(u32::MAX)), Some(u32::MAX));
        assert_eq!(pane_of("pane-"), None);
        assert_eq!(pane_of("7"), None);
        assert_eq!(pane_of("other-7"), None);
    }
}
