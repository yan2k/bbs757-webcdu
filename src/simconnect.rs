//! Minimal SimConnect client (loaded dynamically): just enough to write L-vars.
//! Runs on its own thread; other code sends `(lvar, value)` over a channel.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use libloading::{Library, Symbol};

type Handle = *mut c_void;
type FnOpen =
    unsafe extern "system" fn(*mut Handle, *const c_char, *mut c_void, u32, Handle, u32) -> i32;
type FnClose = unsafe extern "system" fn(Handle) -> i32;
type FnAddDef =
    unsafe extern "system" fn(Handle, u32, *const c_char, *const c_char, u32, f32, u32) -> i32;
type FnSetData = unsafe extern "system" fn(Handle, u32, u32, u32, u32, u32, *const c_void) -> i32;
type FnNextDispatch = unsafe extern "system" fn(Handle, *mut *const u32, *mut u32) -> i32;

const RECV_EXCEPTION: u32 = 1;
const RECV_QUIT: u32 = 3;
const DATATYPE_FLOAT64: u32 = 4;
const OBJECT_ID_USER: u32 = 0;
const UNUSED: u32 = 0xFFFF_FFFF;

/// The SimConnect.dll is embedded so the exe is self-contained.
static DLL_BYTES: &[u8] = include_bytes!("../vendor/SimConnect.dll");

fn dll_path() -> std::io::Result<PathBuf> {
    // Prefer a SimConnect.dll next to the exe (lets you drop in a newer SDK one).
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.with_file_name("SimConnect.dll");
        if p.is_file() {
            return Ok(p);
        }
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("bbs757-webcdu");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join("SimConnect.dll");
    let same = std::fs::read(&p).map(|b| b == DLL_BYTES).unwrap_or(false);
    if !same {
        std::fs::write(&p, DLL_BYTES)?;
    }
    Ok(p)
}

struct Api {
    _lib: &'static Library,
    open: Symbol<'static, FnOpen>,
    close: Symbol<'static, FnClose>,
    add_def: Symbol<'static, FnAddDef>,
    set_data: Symbol<'static, FnSetData>,
    next_dispatch: Symbol<'static, FnNextDispatch>,
}

impl Api {
    fn load() -> Result<Api, String> {
        let path = dll_path().map_err(|e| format!("cannot unpack SimConnect.dll: {e}"))?;
        let lib: &'static Library = Box::leak(Box::new(
            unsafe { Library::new(&path) }
                .map_err(|e| format!("cannot load {}: {e}", path.display()))?,
        ));
        unsafe {
            Ok(Api {
                _lib: lib,
                open: lib.get(b"SimConnect_Open\0").map_err(|e| e.to_string())?,
                close: lib.get(b"SimConnect_Close\0").map_err(|e| e.to_string())?,
                add_def: lib
                    .get(b"SimConnect_AddToDataDefinition\0")
                    .map_err(|e| e.to_string())?,
                set_data: lib
                    .get(b"SimConnect_SetDataOnSimObject\0")
                    .map_err(|e| e.to_string())?,
                next_dispatch: lib
                    .get(b"SimConnect_GetNextDispatch\0")
                    .map_err(|e| e.to_string())?,
            })
        }
    }
}

struct Conn {
    api: Api,
    h: Handle,
    defs: HashMap<String, u32>,
    connected: Arc<AtomicBool>,
}

impl Conn {
    fn open(&mut self) {
        let mut h: Handle = std::ptr::null_mut();
        let name = c"bbs757-webcdu";
        let hr = unsafe {
            (self.api.open)(
                &mut h,
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        if hr == 0 {
            self.h = h;
            self.defs.clear();
            self.connected.store(true, Ordering::Relaxed);
            println!("SimConnect connected");
        }
    }

    fn close(&mut self) {
        if self.connected.swap(false, Ordering::Relaxed) {
            unsafe { (self.api.close)(self.h) };
        }
        self.h = std::ptr::null_mut();
        self.defs.clear();
    }

    fn drain(&mut self) {
        loop {
            let mut p: *const u32 = std::ptr::null();
            let mut n: u32 = 0;
            if unsafe { (self.api.next_dispatch)(self.h, &mut p, &mut n) } != 0 || p.is_null() {
                return;
            }
            // SIMCONNECT_RECV: dwSize, dwVersion, dwID, then (for exceptions) dwException, dwSendID, dwIndex
            let id = unsafe { *p.add(2) };
            if id == RECV_QUIT {
                println!("sim quit");
                self.close();
                return;
            }
            if id == RECV_EXCEPTION && n >= 24 {
                let (exc, send_id) = unsafe { (*p.add(3), *p.add(4)) };
                println!("SimConnect exception {exc} (send id {send_id})");
            }
        }
    }

    fn set_lvar(&mut self, name: &str, value: f64) {
        if !self.connected.load(Ordering::Relaxed) {
            return;
        }
        let next_id = self.defs.len() as u32 + 1;
        let id = match self.defs.get(name) {
            Some(&id) => id,
            None => {
                let datum = CString::new(format!("L:{name}")).unwrap();
                let hr = unsafe {
                    (self.api.add_def)(
                        self.h,
                        next_id,
                        datum.as_ptr(),
                        c"number".as_ptr(),
                        DATATYPE_FLOAT64,
                        0.0,
                        UNUSED,
                    )
                };
                if hr != 0 {
                    println!(
                        "AddToDataDefinition {name} failed ({:#x}) - reconnecting",
                        hr as u32
                    );
                    self.close();
                    return;
                }
                self.defs.insert(name.to_owned(), next_id);
                next_id
            }
        };
        let v = value;
        let hr = unsafe {
            (self.api.set_data)(
                self.h,
                id,
                OBJECT_ID_USER,
                0,
                0,
                8,
                &v as *const f64 as *const c_void,
            )
        };
        if hr != 0 {
            println!("SetData {name} failed ({:#x}) - reconnecting", hr as u32);
            self.close();
        }
    }
}

/// Blocking loop: (re)connects every 3 s while the sim is down, applies L-var writes,
/// drains SimConnect messages.
pub fn run(rx: Receiver<(String, f64)>, connected: Arc<AtomicBool>) {
    let api = match Api::load() {
        Ok(a) => a,
        Err(e) => {
            println!("SimConnect unavailable: {e}");
            return;
        }
    };
    let mut c = Conn {
        api,
        h: std::ptr::null_mut(),
        defs: HashMap::new(),
        connected,
    };
    let mut last_try = Instant::now() - Duration::from_secs(10);
    loop {
        if !c.connected.load(Ordering::Relaxed) && last_try.elapsed() >= Duration::from_secs(3) {
            last_try = Instant::now();
            c.open();
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok((name, v)) => c.set_lvar(&name, v),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if c.connected.load(Ordering::Relaxed) {
            c.drain();
        }
    }
}
