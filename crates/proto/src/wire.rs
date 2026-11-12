//! Messages between the app (or any client) and the daemon, and how they are
//! framed on the socket: a 4-byte little-endian length, then that many bytes
//! of JSON.

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{Frame, InputEvent, Resize};

/// Bumped whenever a message changes shape. A client and daemon with
/// different versions can't talk, except for [`ClientMsg::Hello`], its
/// reply, and [`ClientMsg::Shutdown`], which must never change: they are how
/// an app finds an older daemon and replaces it.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest message accepted, so a bad length can't make us allocate gigabytes.
pub const MAX_MESSAGE: usize = 64 << 20;

pub type SessionId = u32;
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
    /// Start a shell. With `cwd_from`, in that session's directory; with
    /// `argv`, run that instead of the login shell. Replied with
    /// [`Reply::Spawned`].
    Spawn {
        req: ReqId,
        size: Resize,
        cwd_from: Option<SessionId>,
        argv: Option<Vec<String>>,
    },
    /// Replied with [`Reply::Sessions`].
    List {
        req: ReqId,
    },
    /// Send this session's frames and exit to this connection, starting with
    /// a full frame. Takes the session over from another connection.
    Attach {
        session: SessionId,
    },
    Detach {
        session: SessionId,
    },
    /// End the shell and forget the session.
    Close {
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
    /// Replied with [`Reply::Layout`].
    LayoutGet {
        req: ReqId,
    },
    /// Keep the app's layout (its own JSON) while it is closed.
    LayoutSet {
        layout: String,
    },
    /// End every session and exit.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Reply {
    Hello { version: u32, build: String },
    Spawned { session: SessionId },
    Sessions { ids: Vec<SessionId> },
    Layout { layout: Option<String> },
    Error { message: String },
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
    /// The shell exited, with its code when known.
    Exited {
        session: SessionId,
        code: Option<u32>,
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
            ClientMsg::Spawn {
                req: 2,
                size: size(),
                cwd_from: Some(3),
                argv: None,
            },
            ClientMsg::List { req: 3 },
            ClientMsg::Attach { session: 1 },
            ClientMsg::Detach { session: 1 },
            ClientMsg::Close { session: 1 },
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
            ClientMsg::LayoutGet { req: 4 },
            ClientMsg::LayoutSet {
                layout: r#"{"tabs":[]}"#.into(),
            },
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
            reply: Reply::Sessions { ids: vec![1, 2] },
        });
        assert!(matches!(
            reply,
            ServerMsg::Reply {
                req: 1,
                reply: Reply::Sessions { .. }
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
