use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::client::connect;
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

use crate::station::Station;

const ALBUM_COVER_BASE: &str = "https://cdn.listen.moe/covers/";
const ARTIST_IMAGE_BASE: &str = "https://cdn.listen.moe/artists/";

type MetaError = Box<dyn std::error::Error + Send + Sync + 'static>;
type MetaResult<T> = Result<T, MetaError>;

/// Track info sent to the UI thread.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub artist: String,
    pub title: String,
    pub album_cover: Option<String>,
    pub artist_image: Option<String>,
}

#[derive(Debug)]
enum Control {
    Stop,
}

#[derive(Debug)]
enum State {
    Stopped,
    Running { tx: mpsc::Sender<Control> },
}

#[derive(Debug)]
struct Inner {
    station: Station,
    state: State,
    sender: mpsc::Sender<TrackInfo>,
}

#[derive(Debug)]
pub struct Meta {
    inner: RefCell<Inner>,
}

impl Meta {
    pub fn new(station: Station, sender: mpsc::Sender<TrackInfo>) -> Rc<Self> {
        Rc::new(Self {
            inner: RefCell::new(Inner {
                station,
                state: State::Stopped,
                sender,
            }),
        })
    }

    pub fn set_station(&self, station: Station) {
        let mut inner = self.inner.borrow_mut();
        let was_running = matches!(inner.state, State::Running { .. });
        if was_running {
            Self::stop_inner(&mut inner);
        }
        inner.station = station;
        if was_running {
            Self::start_inner(&mut inner);
        }
    }

    pub fn start(&self) {
        let mut inner = self.inner.borrow_mut();
        Self::start_inner(&mut inner);
    }

    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }

    fn start_inner(inner: &mut Inner) {
        match inner.state {
            State::Running { .. } => {
                // Already running.
                return;
            }
            State::Stopped => {
                let (tx, rx) = mpsc::channel::<Control>();
                let station = inner.station;
                let sender = inner.sender.clone();

                inner.state = State::Running { tx: tx.clone() };

                thread::spawn(move || {
                    if let Err(err) = run_meta_loop(station, sender, rx) {
                        eprintln!("Gateway error in metadata loop: {err}");
                    }
                });
            }
        }
    }

    fn stop_inner(inner: &mut Inner) {
        if let State::Running { tx } = &inner.state {
            // Ignore send errors (thread might already be gone).
            let _ = tx.send(Control::Stop);
        }
        inner.state = State::Stopped;
    }
}

impl Drop for Meta {
    fn drop(&mut self) {
        // Best-effort cleanup, same idea as in `Listen`.
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }
}

/// Protocol-level types for the LISTEN.moe gateway

#[derive(Debug, Deserialize)]
struct GatewayHello {
    heartbeat: u64,
}

#[derive(Debug, Deserialize)]
struct GatewaySongPayload {
    song: Song,
}

#[derive(Debug, Deserialize)]
struct Song {
    title: Option<String>,
    #[serde(default)]
    artists: Vec<Artist>,
    #[serde(default)]
    albums: Vec<Album>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: Option<String>,
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Album {
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayEnvelope {
    op: u8,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Value,
}

const OP_HELLO: u8 = 0;
const OP_DISPATCH: u8 = 1;
const OP_HEARTBEAT_ACK: u8 = 10;
const EVENT_TRACK_UPDATE: &str = "TRACK_UPDATE";

/// Outer reconnect loop using blocking tungstenite.
fn run_meta_loop(
    station: Station,
    sender: mpsc::Sender<TrackInfo>,
    rx: mpsc::Receiver<Control>,
) -> MetaResult<()> {
    loop {
        // Before we try a connection, see if we've been asked to stop.
        match rx.try_recv() {
            Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            Err(mpsc::TryRecvError::Empty) => {}
        }

        match run_once(station, sender.clone(), &rx) {
            Ok(()) => {
                // Normal end (server closed the connection).
                match rx.try_recv() {
                    Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    Err(mpsc::TryRecvError::Empty) => {
                        thread::sleep(Duration::from_secs(5));
                    }
                }
            }
            Err(err) => {
                eprintln!("Gateway connection error: {err}, retrying in 5s…");
                // Allow a stop request to cancel the retry delay.
                match rx.try_recv() {
                    Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    Err(mpsc::TryRecvError::Empty) => {
                        thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        }
    }
}

/// Single websocket session, with a simple heartbeat loop.
fn run_once(
    station: Station,
    sender: mpsc::Sender<TrackInfo>,
    rx: &mpsc::Receiver<Control>,
) -> MetaResult<()> {
    // Early stop check.
    match rx.try_recv() {
        Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        Err(mpsc::TryRecvError::Empty) => {}
    }

    let url = station.ws_url();
    let (mut ws, _response) = connect(url)?;
    println!("Gateway connected to LISTEN.moe");

    // Read hello and get heartbeat interval (if any).
    let heartbeat_ms = read_hello_heartbeat(&mut ws)?;
    let heartbeat_dur = heartbeat_ms.map(Duration::from_millis);
    let mut last_heartbeat: Option<Instant> = heartbeat_dur.map(|_| Instant::now());

    loop {
        // Check for control messages first.
        match rx.try_recv() {
            Ok(Control::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        // Heartbeat: if we know an interval, send a heartbeat when it elapses.
        if let (Some(interval), Some(last)) = (heartbeat_dur, last_heartbeat.as_mut()) {
            if last.elapsed() >= interval {
                if let Err(err) = ws.send(Message::Text(r#"{"op":9}"#.into())) {
                    eprintln!("Gateway heartbeat send error: {err}");
                    break;
                }
                *last = Instant::now();
            }
        }

        // Incoming messages.
        let msg = match ws.read() {
            Ok(msg) => msg,
            Err(tungstenite::Error::ConnectionClosed) => break,
            Err(err) => {
                return Err(Box::new(err));
            }
        };

        if !msg.is_text() {
            continue;
        }

        let txt = msg.into_text()?;
        let env: GatewayEnvelope = match serde_json::from_str(&txt) {
            Ok(env) => env,
            Err(err) => {
                eprintln!("Gateway JSON parse error: {err}");
                continue;
            }
        };

        match (env.op, env.t.as_deref()) {
            (OP_HEARTBEAT_ACK, _) => {
                println!("Gateway heartbeat ACK");
            }
            (OP_DISPATCH, Some(EVENT_TRACK_UPDATE)) => {
                if let Some(info) = parse_track_info(&env.d) {
                    let _ = sender.send(info);
                }
            }
            _ => {
                // Ignore other ops/events.
            }
        }
    }

    Ok(())
}

/// Read the initial hello and extract the heartbeat interval (if any).
fn read_hello_heartbeat<S>(ws: &mut WebSocket<S>) -> MetaResult<Option<u64>>
where
    S: Read + Write,
{
    match ws.read() {
        Ok(msg) => {
            if msg.is_text() {
                let txt = msg.into_text()?;
                let env: GatewayEnvelope = serde_json::from_str(&txt)?;

                if env.op == OP_HELLO {
                    let hello: GatewayHello = serde_json::from_value(env.d)?;
                    return Ok(Some(hello.heartbeat));
                }
            }
            Ok(None)
        }
        Err(tungstenite::Error::ConnectionClosed) => Ok(None),
        Err(err) => Err(Box::new(err)),
    }
}

/// Extract artist(s) + title from the gateway payload.
fn parse_track_info(d: &Value) -> Option<TrackInfo> {
    let payload: GatewaySongPayload = serde_json::from_value(d.clone()).ok()?;
    let Song {
        title,
        artists,
        albums,
    } = payload.song;

    let title = title.unwrap_or_else(|| "unknown title".to_owned());

    let artist = if artists.is_empty() {
        "Unknown artist".to_owned()
    } else {
        artists
            .iter()
            .filter_map(|a| a.name.as_deref())
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .join(", ")
    };

    let album_cover = albums
        .first()
        .and_then(|album| album.image.as_deref())
        .map(|name| format!("{ALBUM_COVER_BASE}{name}"));

    let artist_image = artists
        .first()
        .and_then(|a| a.image.as_deref())
        .map(|name| format!("{ARTIST_IMAGE_BASE}{name}"));

    Some(TrackInfo {
        artist,
        title,
        album_cover,
        artist_image,
    })
}
