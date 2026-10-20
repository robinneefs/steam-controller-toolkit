//! Talks to the app's page through WebView2's remote debugging port (Chrome DevTools Protocol):
//! runs JavaScript there, calls the app's commands and reads its snapshots.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub const PORT: u16 = 9222;

/// The app page's DevTools address, if the debugging port is open.
pub fn page_url() -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", PORT)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(s, "GET /json HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\n\r\n").ok()?;
    // Read what arrives; the server may keep the connection open, so stop at the body's length.
    let mut bytes = vec![];
    let mut buf = [0u8; 8192];
    loop {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..i]).to_ascii_lowercase();
            let len = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()));
            if len.is_some_and(|len| bytes.len() >= i + 4 + len) {
                break;
            }
        }
    }
    let body = String::from_utf8_lossy(&bytes).to_string();
    let list: Value = serde_json::from_str(&body[body.find("\r\n\r\n")? + 4..]).ok()?;
    let page = list.as_array()?.iter().find(|t| t["type"] == "page")?;
    page["webSocketDebuggerUrl"].as_str().map(String::from)
}

pub struct Page {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    id: u64,
}

impl Page {
    pub fn connect() -> Result<Page, String> {
        let url = page_url().ok_or("the app's window isn't open with the debugging port")?;
        let (ws, _) = tungstenite::connect(url).map_err(|e| format!("couldn't reach the app's page: {e}"))?;
        Ok(Page { ws, id: 0 })
    }

    /// Runs `js` in the page (awaiting a promise) and returns its value.
    pub fn eval(&mut self, js: &str) -> Result<Value, String> {
        self.id += 1;
        let id = self.id;
        let msg = json!({ "id": id, "method": "Runtime.evaluate", "params": { "expression": js, "awaitPromise": true, "returnByValue": true } });
        self.ws.send(Message::Text(msg.to_string())).map_err(|e| e.to_string())?;
        loop {
            let Message::Text(text) = self.ws.read().map_err(|e| e.to_string())? else { continue };
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if v["id"] != id {
                continue;
            }
            if let Some(err) = v["result"]["exceptionDetails"].as_object() {
                return Err(err.get("exception").and_then(|e| e["description"].as_str()).unwrap_or("error in the page").to_string());
            }
            return Ok(v["result"]["result"]["value"].clone());
        }
    }

    /// Calls one of the app's Tauri commands.
    pub fn invoke(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        self.eval(&format!("window.__TAURI_INTERNALS__.invoke({}, {args})", json!(cmd)))
    }

    /// The latest snapshot the app sent (`controller` event), listening first if needed.
    pub fn snapshot(&mut self) -> Result<Value, String> {
        let js = "(async()=>{const T=window.__TAURI_INTERNALS__;
            if(!window.__rc){window.__rc=1; await T.invoke('plugin:event|listen',{event:'controller',target:{kind:'Any'},handler:T.transformCallback(e=>{window.__rcSnap=e.payload})});}
            for(let i=0;i<50&&!window.__rcSnap;i++) await new Promise(r=>setTimeout(r,20));
            return window.__rcSnap ?? null})()";
        let v = self.eval(js)?;
        if v.is_null() {
            Err("no snapshots from the app (is the window minimised?)".into())
        } else {
            Ok(v)
        }
    }

    /// Shows a tab, as clicking it in the sidebar does.
    pub fn show_tab(&mut self, tab: &str) -> Result<(), String> {
        self.eval(&format!("document.querySelector('nav [data-tab={tab}]')?.click(); 1")).map(|_| ())
    }
}
