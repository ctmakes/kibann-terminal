//! Messages between the app (or any client) and the daemon, and how they are
//! framed on the socket: a 4-byte little-endian length, then that many bytes
//! of JSON.

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use std::collections::BTreeMap;

use crate::git::{Diff, GitInfo, PaneDir};
use crate::layout::{Action, Layout, PaneId, TabId};
use crate::status::{AgentState, PaneStatus};
use crate::{Frame, InputEvent, Resize};

/// Bumped whenever a message changes shape. A client and daemon with
/// different versions can't talk, except for [`ClientMsg::Hello`], its
/// reply, and [`ClientMsg::Shutdown`], which must never change: they are how
/// an app finds an older daemon and replaces it.
pub const PROTOCOL_VERSION: u32 = 6;

/// Largest message accepted, so a bad length can't make us allocate gigabytes.
pub const MAX_MESSAGE: usize = 64 << 20;

/// A session is a pane: the same id.
pub type SessionId = PaneId;
/// Matches a reply to its request.
pub type ReqId = u32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message on a connection. Replied with [`Reply::Hello`].
    Hello {
        req: ReqId,
        version: u32,
    },
    /// Replied with [`Reply::Layout`], and the same layout as the first
    /// [`ServerMsg::Layout`]; from then on every change comes as one, except
    /// the focus changes this connection makes itself. Agent status and git
    /// context come the same way, as [`ServerMsg::Status`] and
    /// [`ServerMsg::Git`].
    Watch {
        req: ReqId,
    },
    /// Replied with [`Reply::Layout`].
    GetLayout {
        req: ReqId,
    },
    /// Change the layout. Replied with [`Reply::Acted`].
    Act {
        req: ReqId,
        action: Action,
    },
    /// Title and directory of every pane. Replied with [`Reply::Panes`].
    Panes {
        req: ReqId,
    },
    /// The pane's text. Replied with [`Reply::Text`].
    Read {
        req: ReqId,
        pane: PaneId,
        source: ReadSource,
        /// Only the last this many lines.
        lines: Option<u32>,
    },
    /// Whether the pane's program ended, and whether `mark` was printed
    /// there (see [`MARK_OSC`]). Replied with [`Reply::Status`].
    Status {
        req: ReqId,
        pane: PaneId,
        mark: Option<String>,
    },
    /// An agent in `pane` says what it is doing; this beats what the
    /// screen shows until [`ClientMsg::Release`], or until the pane's shell
    /// is back in the foreground. A `seq` not above the last one is
    /// ignored. Replied with [`Reply::Ok`].
    Report {
        req: ReqId,
        pane: PaneId,
        agent: String,
        state: AgentState,
        seq: Option<u64>,
    },
    /// Everything uncommitted in `pane`'s checkout. Replied with
    /// [`Reply::Diff`].
    Diff {
        req: ReqId,
        pane: PaneId,
    },
    /// Drop `pane`'s report. Replied with [`Reply::Ok`].
    Release {
        req: ReqId,
        pane: PaneId,
    },
    /// Color every session (and every new one) with `theme`. Replied with
    /// [`Reply::Ok`].
    SetTheme {
        req: ReqId,
        theme: crate::theme::Theme,
    },
    /// Send this session's frames to this connection, starting with a full
    /// frame. Takes the session over from another connection.
    Attach {
        session: SessionId,
    },
    Detach {
        session: SessionId,
    },
    Input {
        session: SessionId,
        event: InputEvent,
    },
    Resize {
        session: SessionId,
        size: Resize,
    },
    Ack {
        session: SessionId,
        seq: u64,
    },
    RequestFull {
        session: SessionId,
    },
    /// End every session and exit.
    Shutdown,
}

/// What [`ClientMsg::Read`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadSource {
    /// The screen as the program drew it, row by row.
    Visible,
    /// Scrollback and screen, with lines that wrapped joined again.
    Recent,
}

/// An OSC number nothing else uses. `ESC ] 7770 ; <mark> ; <code> BEL` in a
/// pane's output records `mark` with exit code `code`, for
/// [`ClientMsg::Status`]; the emulator ignores it, so nothing shows.
pub const MARK_OSC: u32 = 7770;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: PaneId,
    pub tab: TabId,
    pub title: String,
    pub cwd: Option<String>,
    /// The agent in the pane and its state, if there is one.
    pub agent: Option<String>,
    pub state: Option<AgentState>,
    /// The git checkout the pane is in, if any.
    pub git: Option<GitInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Reply {
    Hello {
        version: u32,
        build: String,
    },
    Layout {
        layout: Layout,
    },
    /// The pane and tab an action made or acted on.
    Acted {
        pane: Option<PaneId>,
        tab: Option<TabId>,
    },
    Panes {
        panes: Vec<PaneInfo>,
    },
    Text {
        text: String,
    },
    Status {
        /// The program exited (the pane is gone), with its code if known.
        ended: bool,
        code: Option<u32>,
        /// The code that came with the mark, once it was printed.
        mark: Option<i32>,
        /// The agent in the pane, if any.
        agent: Option<PaneStatus>,
    },
    Ok,
    Diff {
        diff: Diff,
    },
    Error {
        message: String,
    },
}

/// From the daemon. `F` is [`Frame`] when sending; a client that only passes
/// frames on can read them as raw JSON with [`RawServerMsg`]. Externally
/// tagged (`{"frame": {...}}`), because serde can't keep raw JSON inside an
/// internally tagged enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerMsg<F = Frame> {
    Reply {
        req: ReqId,
        reply: Reply,
    },
    Frame {
        session: SessionId,
        frame: F,
    },
    /// The layout changed (to connections that sent [`ClientMsg::Watch`]).
    Layout {
        layout: Layout,
    },
    /// Every pane with an agent in it, whenever one changes (to watchers,
    /// first right after the first layout).
    Status {
        panes: BTreeMap<PaneId, PaneStatus>,
    },
    /// Every pane's directory and git checkout, whenever one changes (to
    /// watchers, first right after the first layout).
    Git {
        panes: BTreeMap<PaneId, PaneDir>,
    },
}

/// A server message with the frame left as JSON text.
pub type RawServerMsg = ServerMsg<Box<RawValue>>;

/// Write one message.
pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let body = serde_json::to_vec(msg).map_err(io::Error::other)?;
    if body.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf)?;
    w.flush()
}

/// Read one message's bytes. `None` when the peer closed the connection
/// between messages.
pub fn read_raw<R: Read>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Read and decode one message. `None` when the peer closed the connection.
pub fn read_msg<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>> {
    match read_raw(r)? {
        Some(body) => serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cursor, CursorStyle, MouseTracking, Scrollbar};

    fn size() -> Resize {
        Resize {
            cols: 80,
            rows: 24,
            cell_w: 8,
            cell_h: 16,
        }
    }

    fn frame() -> Frame {
        Frame {
            seq: 7,
            full: true,
            cols: 80,
            rows_total: 24,
            default_fg: 0xffffff,
            default_bg: 0,
            styles: vec![],
            rows: vec![],
            cursor: Cursor {
                x: 1,
                y: 2,
                in_viewport: true,
                visible: true,
                blinking: false,
                style: CursorStyle::Block,
                wide: false,
                color: None,
            },
            mouse_tracking: MouseTracking::None,
            focus_reporting: false,
            scrollbar: Scrollbar::default(),
            selection_text: None,
            title: Some("zsh".into()),
            link: None,
        }
    }

    fn round_trip<T: Serialize + DeserializeOwned>(msg: &T) -> T {
        let mut buf = Vec::new();
        write_msg(&mut buf, msg).unwrap();
        let mut r = buf.as_slice();
        let out = read_msg(&mut r).unwrap().unwrap();
        assert!(r.is_empty());
        out
    }

    #[test]
    fn client_messages_round_trip() {
        let msgs = vec![
            ClientMsg::Hello {
                req: 1,
                version: PROTOCOL_VERSION,
            },
            ClientMsg::Watch { req: 2 },
            ClientMsg::Act {
                req: 3,
                action: Action::Split {
                    pane: 1,
                    dir: crate::layout::Dir::Row,
                    cwd: None,
                    argv: None,
                    focus: false,
                },
            },
            ClientMsg::Read {
                req: 4,
                pane: 1,
                source: ReadSource::Recent,
                lines: Some(10),
            },
            ClientMsg::Attach { session: 1 },
            ClientMsg::Detach { session: 1 },
            ClientMsg::Input {
                session: 1,
                event: InputEvent::Text {
                    data: "ls\r".into(),
                },
            },
            ClientMsg::Resize {
                session: 1,
                size: size(),
            },
            ClientMsg::Ack { session: 1, seq: 9 },
            ClientMsg::RequestFull { session: 1 },
            ClientMsg::Shutdown,
        ];
        for m in msgs {
            assert_eq!(round_trip(&m), m);
        }
    }

    #[test]
    fn frames_can_be_read_as_raw_json() {
        let mut buf = Vec::new();
        write_msg(
            &mut buf,
            &ServerMsg::Frame {
                session: 5,
                frame: frame(),
            },
        )
        .unwrap();
        let raw: RawServerMsg = read_msg(&mut buf.as_slice()).unwrap().unwrap();
        let ServerMsg::Frame {
            session,
            frame: json,
        } = raw
        else {
            panic!("not a frame");
        };
        assert_eq!(session, 5);
        let back: Frame = serde_json::from_str(json.get()).unwrap();
        assert_eq!(back, frame());

        let reply: ServerMsg = round_trip(&ServerMsg::<Frame>::Reply {
            req: 1,
            reply: Reply::Acted {
                pane: Some(2),
                tab: None,
            },
        });
        assert!(matches!(
            reply,
            ServerMsg::Reply {
                req: 1,
                reply: Reply::Acted { pane: Some(2), .. }
            }
        ));
    }

    #[test]
    fn end_of_stream_and_bad_lengths() {
        let empty: io::Result<Option<ClientMsg>> = read_msg(&mut [].as_slice());
        assert!(empty.unwrap().is_none());

        // Cut off inside the body.
        let mut buf = Vec::new();
        write_msg(&mut buf, &ClientMsg::Shutdown).unwrap();
        buf.pop();
        assert!(read_msg::<_, ClientMsg>(&mut buf.as_slice()).is_err());

        // A length past the cap is refused before allocating.
        let huge = ((MAX_MESSAGE + 1) as u32).to_le_bytes();
        let e = read_raw(&mut huge.as_slice()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }
}
