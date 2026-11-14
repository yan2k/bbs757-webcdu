//! BBS 757 web CDU.
//!
//! - Pretends to be MobiFlight's CDU websocket hub on 127.0.0.1:8320. The BlueBird 757 connects
//!   there by itself (/winwing/cdu-captain, /winwing/cdu-co-pilot) and pushes each CDU screen as
//!   {"Target":"Display","Data":[14*24 cells of [char, colour, small, inverted]]}.
//! - Serves the web CDU on 0.0.0.0:8757 and relays the screens to it.
//! - Key presses from the page set L:B757_CDU_<side>_<KEY> (1 held / 0 released) via SimConnect.
//!
//! MobiFlight must not be running at the same time (it owns port 8320).

mod simconnect;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tokio::time::{sleep, sleep_until, timeout, Instant};

const AIRCRAFT_ADDR: &str = "127.0.0.1:8320";
const DEFAULT_WEB_PORT: u16 = 8757;
const MIN_HOLD: Duration = Duration::from_millis(80); // key L-var stays 1 at least this long
const REPEAT_GAP: Duration = Duration::from_millis(40); // pause between consecutive keys
const STUCK_TIMEOUT: Duration = Duration::from_secs(5); // release a key whose "up" never came

static INDEX_HTML: &str = include_str!("../web/index.html");
static FONT_MONO: &[u8] = include_bytes!("../assets/fonts/B612Mono-Regular.ttf");
static FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/B612-Bold.ttf");

fn valid_key(k: &str) -> bool {
    const NAMED: &[&str] = &[
        "INIT", "RTE", "DEPARR", "ATC", "VNAV", "FIX", "LEGS", "HOLD", "FMCCOMM", "PROG", "EXEC",
        "MENU", "NAVRAD", "PREV", "NEXT", "NUMPER", "NUMPLUS", "KEYSP", "KEYDEL", "KEYSLASH",
        "KEYCLR",
    ];
    if NAMED.contains(&k) {
        return true;
    }
    let b = k.as_bytes();
    match b {
        [b'L' | b'R', d] => (b'1'..=b'6').contains(d),
        [b'N', b'U', b'M', d] => d.is_ascii_digit(),
        [b'K', b'E', b'Y', c] => c.is_ascii_uppercase(),
        _ => false,
    }
}

fn seat_side(seat: &str) -> Option<u8> {
    match seat {
        "cdu-captain" => Some(1),
        "cdu-co-pilot" => Some(2),
        "cdu-observer" => Some(3),
        _ => None,
    }
}

// ------------------------------------------------------------------------------------ keys
type KeyJob = (String, Option<Arc<Notify>>);

struct Keys {
    sc: std_mpsc::Sender<(String, f64)>,
    queues: Mutex<HashMap<u8, mpsc::UnboundedSender<KeyJob>>>,
    pending_up: Mutex<HashMap<(u8, String), Arc<Notify>>>,
}

impl Keys {
    fn submit(self: &Arc<Self>, side: u8, key: &str, action: &str) {
        if action == "up" {
            if let Some(n) = self
                .pending_up
                .lock()
                .unwrap()
                .remove(&(side, key.to_owned()))
            {
                n.notify_one(); // stores a permit if the worker hasn't started waiting yet
            }
            return;
        }
        let ev = (action == "down").then(|| {
            let n = Arc::new(Notify::new());
            self.pending_up
                .lock()
                .unwrap()
                .insert((side, key.to_owned()), n.clone());
            n
        });
        let mut qs = self.queues.lock().unwrap();
        let tx = qs.entry(side).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            tokio::spawn(self.clone().worker(side, rx));
            tx
        });
        let _ = tx.send((key.to_owned(), ev));
    }

    /// One key at a time per CDU: the 757 only registers one held key per frame, so overlapping
    /// presses (fast typing) would get dropped.
    async fn worker(self: Arc<Self>, side: u8, mut rx: mpsc::UnboundedReceiver<KeyJob>) {
        while let Some((key, ev)) = rx.recv().await {
            let lvar = format!("B757_CDU_{side}_{key}");
            let _ = self.sc.send((lvar.clone(), 1.0));
            let t0 = Instant::now();
            if let Some(n) = ev {
                if timeout(STUCK_TIMEOUT, n.notified()).await.is_err() {
                    println!("{lvar} held > {}s, releasing", STUCK_TIMEOUT.as_secs());
                    let mut p = self.pending_up.lock().unwrap();
                    if p.get(&(side, key.clone()))
                        .is_some_and(|x| Arc::ptr_eq(x, &n))
                    {
                        p.remove(&(side, key.clone()));
                    }
                }
            }
            sleep_until(t0 + MIN_HOLD).await;
            let _ = self.sc.send((lvar, 0.0));
            sleep(REPEAT_GAP).await;
        }
    }
}

// ------------------------------------------------------------------------------------ hub
struct Hub {
    keys: Arc<Keys>,
    sim: Arc<AtomicBool>,
    tx: broadcast::Sender<Arc<str>>,
    screens: Mutex<BTreeMap<u8, Arc<str>>>,
    aircraft: Mutex<BTreeMap<u8, usize>>, // side -> open connections
    lights: watch::Receiver<simconnect::Lights>,
}

impl Hub {
    fn status(&self) -> Arc<str> {
        let ac: Vec<u8> = self
            .aircraft
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, &n)| n > 0)
            .map(|(&s, _)| s)
            .collect();
        serde_json::json!({"t": "status", "aircraft": ac, "sim": self.sim.load(Ordering::Relaxed)})
            .to_string()
            .into()
    }
    fn lights(&self) -> Arc<str> {
        let l = *self.lights.borrow();
        serde_json::json!({"t": "lights", "cdu": l})
            .to_string()
            .into()
    }
    /// Everything a freshly connected (or lagging) page needs.
    fn snapshot(&self) -> Vec<Arc<str>> {
        let mut v = vec![self.status(), self.lights()];
        v.extend(self.screens.lock().unwrap().values().cloned());
        v
    }
    fn publish(&self, m: Arc<str>) {
        let _ = self.tx.send(m);
    }
}

type AppState = Arc<Hub>;

async fn aircraft_ws(
    ws: WebSocketUpgrade,
    Path(seat): Path<String>,
    State(hub): State<AppState>,
) -> impl IntoResponse {
    ws.max_message_size(16 << 20)
        .on_upgrade(move |socket| aircraft_conn(socket, seat, hub))
}

async fn aircraft_conn(mut socket: WebSocket, seat: String, hub: AppState) {
    let Some(side) = seat_side(&seat) else {
        println!("ignoring unknown aircraft path /winwing/{seat}");
        return;
    };
    println!("aircraft connected: {seat}");
    *hub.aircraft.lock().unwrap().entry(side).or_default() += 1;
    hub.publish(hub.status());

    while let Some(Ok(msg)) = socket.recv().await {
        let Message::Text(text) = msg else { continue };
        let text = text.as_str();
        if !text.contains("\"Display\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        let Some(data) = v.get("Data").filter(|d| d.is_array()) else {
            continue;
        };
        let frame: Arc<str> = serde_json::json!({"t": "display", "side": side, "data": data})
            .to_string()
            .into();
        let changed = {
            let mut s = hub.screens.lock().unwrap();
            if s.get(&side).is_some_and(|old| **old == *frame) {
                false
            } else {
                s.insert(side, frame.clone());
                true
            }
        };
        if changed {
            hub.publish(frame);
        }
    }

    println!("aircraft disconnected: {seat}");
    if let Some(n) = hub.aircraft.lock().unwrap().get_mut(&side) {
        *n = n.saturating_sub(1);
    }
    hub.publish(hub.status());
}

#[derive(Deserialize)]
struct KeyMsg {
    side: u8,
    k: String,
    #[serde(default = "tap")]
    a: String,
}
fn tap() -> String {
    "tap".into()
}

async fn browser_ws(ws: WebSocketUpgrade, State(hub): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| browser_conn(socket, hub))
}

async fn browser_conn(socket: WebSocket, hub: AppState) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = hub.tx.subscribe();
    let initial = hub.snapshot();

    let out_hub = hub.clone();
    let mut out = tokio::spawn(async move {
        for m in initial {
            sink.send(Message::Text((&*m).into())).await?;
        }
        let mut ping = tokio::time::interval(Duration::from_secs(20));
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Ok(m) => sink.send(Message::Text((&*m).into())).await?,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // fell behind: just resend the current state
                        for m in out_hub.snapshot() { sink.send(Message::Text((&*m).into())).await?; }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = ping.tick() => sink.send(Message::Ping(Default::default())).await?,
            }
        }
        Ok::<_, axum::Error>(())
    });

    let mut held: HashSet<(u8, String)> = HashSet::new();
    loop {
        tokio::select! {
            msg = stream.next() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(text) = msg else { continue };
                let Ok(m) = serde_json::from_str::<KeyMsg>(text.as_str()) else { continue };
                if !(m.side == 1 || m.side == 2) || !valid_key(&m.k) || !matches!(m.a.as_str(), "down" | "up" | "tap") {
                    continue;
                }
                match m.a.as_str() {
                    "down" => { held.insert((m.side, m.k.clone())); }
                    "up" => { held.remove(&(m.side, m.k.clone())); }
                    _ => {}
                }
                hub.keys.submit(m.side, &m.k, &m.a);
            }
            _ = &mut out => break,
        }
    }
    out.abort();
    for (side, key) in held {
        hub.keys.submit(side, &key, "up"); // page closed mid-press
    }
}

async fn index() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        INDEX_HTML,
    )
}
async fn font_mono() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "font/ttf"),
            (header::CACHE_CONTROL, "max-age=604800"),
        ],
        FONT_MONO,
    )
}
async fn font_bold() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "font/ttf"),
            (header::CACHE_CONTROL, "max-age=604800"),
        ],
        FONT_BOLD,
    )
}

fn fail(msg: String) -> ! {
    eprintln!("\nERROR: {msg}\n\nPress Enter to exit.");
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let web_port: u16 = std::env::var("CDU_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_WEB_PORT);
    println!("bbs757-webcdu v{}", env!("CARGO_PKG_VERSION"));

    let sim = Arc::new(AtomicBool::new(false));
    let (sc_tx, sc_rx) = std_mpsc::channel();
    let (lights_tx, lights_rx) = watch::channel(simconnect::Lights::default());
    {
        let sim = sim.clone();
        std::thread::spawn(move || simconnect::run(sc_rx, sim, lights_tx));
    }

    let keys = Arc::new(Keys {
        sc: sc_tx,
        queues: Mutex::default(),
        pending_up: Mutex::default(),
    });
    let (tx, _) = broadcast::channel(64);
    let hub: AppState = Arc::new(Hub {
        keys,
        sim,
        tx,
        screens: Mutex::default(),
        aircraft: Mutex::default(),
        lights: lights_rx,
    });

    // push EXEC / annunciator changes to pages
    {
        let hub = hub.clone();
        let mut rx = hub.lights.clone();
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                hub.publish(hub.lights());
            }
        });
    }

    // push status changes (e.g. sim connect/disconnect) to pages
    {
        let hub = hub.clone();
        tokio::spawn(async move {
            let mut last: Option<Arc<str>> = None;
            loop {
                let s = hub.status();
                if last.as_deref() != Some(&*s) {
                    hub.publish(s.clone());
                    last = Some(s);
                }
                sleep(Duration::from_secs(1)).await;
            }
        });
    }

    let air = Router::new()
        .route("/winwing/{seat}", get(aircraft_ws))
        .with_state(hub.clone());
    let page = Router::new()
        .route("/", get(index))
        .route("/ws", get(browser_ws))
        .route("/fonts/B612Mono-Regular.ttf", get(font_mono))
        .route("/fonts/B612-Bold.ttf", get(font_bold))
        .with_state(hub.clone());

    let air_l = tokio::net::TcpListener::bind(AIRCRAFT_ADDR).await.unwrap_or_else(|e| {
        fail(format!("cannot listen on {AIRCRAFT_ADDR} ({e}).\nIs MobiFlight or another copy of this app running?"))
    });
    let web_addr = SocketAddr::from(([0, 0, 0, 0], web_port));
    let web_l = tokio::net::TcpListener::bind(web_addr)
        .await
        .unwrap_or_else(|e| fail(format!("cannot listen on {web_addr} ({e}).")));
    println!("aircraft hub on {AIRCRAFT_ADDR}, web CDU on http://{web_addr}/");

    let a = tokio::spawn(async move { axum::serve(air_l, air).await });
    let w = tokio::spawn(async move { axum::serve(web_l, page).await });
    let _ = tokio::join!(a, w);
}
