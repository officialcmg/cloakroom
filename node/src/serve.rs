//! `cloakroom serve`: the local app. Serves the UI and a small JSON API on 127.0.0.1 over this
//! party's directory, and runs the maker or taker as a child process. Keys and swap secrets never
//! leave the machine: the API reads the same swap files the node writes, with secrets removed.
//!
//! Requests must name localhost in `Host` (no DNS rebinding) and every POST must carry
//! `X-Cloakroom: 1` (a header other sites cannot send without a CORS preflight, which this server
//! never answers), so a web page open in the same browser cannot start or stop a swap.

use crate::{
    common::{self, Config},
    evm::Evm,
    wallet,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiny_http::{Header, Method, Request, Response, Server};

pub struct Opts {
    pub port: u16,
    pub ui: PathBuf,
    pub ds: PathBuf,
}

/// Fields of a swap record the API never returns.
const SECRET: &[&str] = &["payout_key", "a", "b", "s_t", "s_m", "e_t", "e_m", "joint_fvk", "commit_s_t"];

#[derive(Default)]
struct State {
    child: Option<(Child, String, PathBuf)>,
    last_exit: Option<String>,
    balance: Value,
    evm: Value,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn run(dir: &Path, opts: Opts) -> Result<()> {
    let cfg: Config = common::load(&dir.join("config.json"))?;
    if let Some(n) = &cfg.network {
        wallet::set_network(n)?;
    }
    let role: &'static str = if cfg.evm_key.is_some() { "maker" } else { "taker" };
    let ua = common::Me::init(dir)?.ua();
    let st = Arc::new(Mutex::new(State::default()));
    {
        let (st, dir, cfg) = (st.clone(), dir.to_path_buf(), cfg.clone());
        thread::spawn(move || loop {
            refresh(&dir, &cfg, role, &st);
            thread::sleep(Duration::from_secs(10));
        });
    }
    let server = Arc::new(Server::http(("127.0.0.1", opts.port)).map_err(|e| anyhow!("listen on 127.0.0.1:{}: {e}", opts.port))?);
    println!("Cloakroom {role} app: http://127.0.0.1:{}/", opts.port);
    // A few workers, so one slow request (a wallet read, a held-open connection) never stalls the page.
    let (cfg, ua, opts) = (Arc::new(cfg), Arc::new(ua), Arc::new(opts));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (server, st, cfg, ua, opts, dir) = (server.clone(), st.clone(), cfg.clone(), ua.clone(), opts.clone(), dir.to_path_buf());
            thread::spawn(move || {
                while let Ok(mut req) = server.recv() {
                    let reply = handle(&mut req, &dir, &cfg, role, &ua, &opts, &st);
                    let _ = match reply {
                        Ok(r) => req.respond(r),
                        Err(e) => req.respond(json_resp(500, json!({ "error": e.to_string() }))),
                    };
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

/// Balances change slowly and a wallet sync is not free, so they are refreshed in the background.
fn refresh(dir: &Path, cfg: &Config, role: &str, st: &Arc<Mutex<State>>) {
    let exe = std::env::current_exe().ok();
    let balance = exe
        .and_then(|e| Command::new(e).args(["balance", "--dir"]).arg(dir).output().ok())
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
        .map(|mut v| {
            v["at"] = json!(now());
            v
        });
    let mut evm_info = Value::Null;
    if role == "maker" {
        if let Some(key) = &cfg.evm_key {
            let e = Evm { rpc: cfg.evm_rpc.clone(), escrow: cfg.escrow.to_lowercase(), token: cfg.token.to_lowercase(), from_block: cfg.evm_from_block.unwrap_or(0) };
            if let Ok(addr) = Evm::address_of(key) {
                let token = e.token_balance(&addr).ok();
                let eth = Command::new("cast").args(["balance", "--rpc-url", &cfg.evm_rpc, &addr]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
                evm_info = json!({ "address": addr, "token_units": token, "eth_wei": eth, "at": now() });
            }
        }
    }
    let mut s = st.lock().unwrap();
    if let Some(b) = balance {
        s.balance = b;
    }
    if !evm_info.is_null() {
        s.evm = evm_info;
    }
}

fn handle(req: &mut Request, dir: &Path, cfg: &Config, role: &str, ua: &str, opts: &Opts, st: &Arc<Mutex<State>>) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let host_ok = req.headers().iter().any(|h| {
        h.field.equiv("Host") && {
            let v = h.value.as_str();
            v == format!("127.0.0.1:{}", opts.port) || v == format!("localhost:{}", opts.port)
        }
    });
    if !host_ok {
        return Ok(json_resp(403, json!({ "error": "this app only answers on localhost" })));
    }
    let url = req.url().split('?').next().unwrap_or("/").to_string();
    if *req.method() == Method::Post && !req.headers().iter().any(|h| h.field.equiv("X-Cloakroom") && h.value.as_str() == "1") {
        return Ok(json_resp(403, json!({ "error": "missing X-Cloakroom header" })));
    }
    match (req.method(), url.as_str()) {
        (Method::Get, "/api/status") => Ok(json_resp(200, status(dir, cfg, role, ua, st))),
        (Method::Get, "/api/swaps") => Ok(json_resp(200, json!({ "swaps": swaps(dir) }))),
        (Method::Get, "/api/log") => Ok(json_resp(200, json!({ "lines": log_tail(st, 40) }))),
        (Method::Post, "/api/swaps") if role == "taker" => {
            let body = body_json(req)?;
            let lots = body["lots"].as_u64().filter(|l| (1..=255).contains(l)).ok_or_else(|| anyhow!("lots must be 1 to 255"))?;
            start(dir, st, "taker", &["--lots", &lots.to_string()])
        }
        (Method::Post, "/api/maker/start") if role == "maker" => start(dir, st, "maker", &[]),
        (Method::Post, "/api/stop") => {
            let mut s = st.lock().unwrap();
            if let Some((mut c, r, _)) = s.child.take() {
                let _ = c.kill();
                let _ = c.wait();
                s.last_exit = Some(format!("{r} stopped by you"));
            }
            Ok(json_resp(200, json!({ "ok": true })))
        }
        (Method::Get, p) if !p.starts_with("/api/") => static_file(p, opts),
        _ => Ok(json_resp(404, json!({ "error": "not found" }))),
    }
}

fn status(dir: &Path, cfg: &Config, role: &str, ua: &str, st: &Arc<Mutex<State>>) -> Value {
    let mut s = st.lock().unwrap();
    let running = match s.child.as_mut() {
        Some((c, r, _)) => match c.try_wait() {
            Ok(None) => Some(r.clone()),
            Ok(Some(code)) => {
                let r = r.clone();
                s.child = None;
                // A clean exit is reported by the swap records themselves; only a failure needs a notice.
                s.last_exit = (!code.success()).then(|| format!("The {r} process stopped with an error ({code}). See the technical log."));
                None
            }
            Err(_) => None,
        },
        None => None,
    };
    let r = cfg.reorg_budget;
    json!({
        "role": role,
        "ua": ua,
        "dir": dir.file_name().map(|n| n.to_string_lossy().to_string()),
        "network": cfg.network.clone().unwrap_or_else(|| "regtest".into()),
        "running": running,
        "last_exit": s.last_exit,
        "balance": s.balance,
        "evm": s.evm,
        "now": now(),
        "params": {
            "lot_zats": cfg.lot_zats,
            "price_per_lot": cfg.price_per_lot,
            "relayer_fee": cfg.relayer_fee,
            "premium_zats": cfg.premium_zats,
            "max_price": cfg.max_price,
            "max_premium": cfg.max_premium,
            "max_lots": cfg.max_lots,
            "inclusion_secs": cfg.inclusion_secs,
            "finality_secs": cfg.finality_secs,
            "reorg_budget": r,
            "budget_secs": (r + 1) * cfg.inclusion_secs + r * cfg.finality_secs,
            "k_z": cfg.k_z,
            "cover_max_secs": cfg.cover_max_secs.unwrap_or(0),
            "escrow": cfg.escrow,
            "token": cfg.token,
            "maker_evm": cfg.maker_evm,
            "maker_ua": cfg.maker_ua,
        },
    })
}

fn swaps(dir: &Path) -> Vec<Value> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(dir.join("swaps")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x != "json").unwrap_or(true) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            let Ok(mut v) = serde_json::from_str::<Value>(&text) else { continue };
            if let Some(m) = v.as_object_mut() {
                for k in SECRET {
                    m.remove(*k);
                }
                let updated = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
                m.insert("updated".into(), json!(updated));
            }
            out.push(v);
        }
    }
    out.sort_by_key(|v| std::cmp::Reverse(v["updated"].as_u64().unwrap_or(0)));
    out
}

fn start(dir: &Path, st: &Arc<Mutex<State>>, role: &str, extra: &[&str]) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let mut s = st.lock().unwrap();
    if let Some((c, r, _)) = s.child.as_mut() {
        if matches!(c.try_wait(), Ok(None)) {
            return Ok(json_resp(409, json!({ "error": format!("the {r} is already running") })));
        }
    }
    fs::create_dir_all(dir.join("logs"))?;
    let log = dir.join("logs").join(format!("app-{role}-{}.log", now()));
    let f = fs::File::create(&log)?;
    let child = Command::new(std::env::current_exe()?)
        .arg(role)
        .arg("--dir")
        .arg(dir)
        .args(extra)
        .stdout(Stdio::from(f.try_clone()?))
        .stderr(Stdio::from(f))
        .spawn()?;
    s.child = Some((child, role.to_string(), log));
    s.last_exit = None;
    Ok(json_resp(200, json!({ "ok": true, "running": role })))
}

/// The last lines of the running (or last) process's log, stage lines only, secrets never printed there.
fn log_tail(st: &Arc<Mutex<State>>, n: usize) -> Vec<String> {
    let s = st.lock().unwrap();
    let Some((_, _, path)) = s.child.as_ref() else { return vec![] };
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<String> = text.lines().filter(|l| l.starts_with('[')).map(|l| l.to_string()).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

fn body_json(req: &mut Request) -> Result<Value> {
    let mut b = String::new();
    req.as_reader().take(64 * 1024).read_to_string(&mut b)?;
    Ok(if b.trim().is_empty() { json!({}) } else { serde_json::from_str(&b)? })
}

fn static_file(path: &str, opts: &Opts) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let (root, rel) = match path.strip_prefix("/ds/") {
        Some(rest) => (&opts.ds, rest.to_string()),
        None => (&opts.ui, if path == "/" { "index.html".to_string() } else { path.trim_start_matches('/').to_string() }),
    };
    if rel.split('/').any(|seg| seg.is_empty() || seg == ".." || seg.starts_with('.')) {
        return Ok(json_resp(404, json!({ "error": "not found" })));
    }
    let full = root.join(&rel);
    let Ok(bytes) = fs::read(&full) else { return Ok(json_resp(404, json!({ "error": "not found" }))) };
    let ct = match full.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    Ok(with_headers(Response::from_data(bytes), ct))
}

fn json_resp(code: u16, v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    with_headers(Response::from_data(serde_json::to_vec(&v).unwrap_or_default()).with_status_code(code), "application/json")
}

fn with_headers(r: Response<std::io::Cursor<Vec<u8>>>, ct: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let h = |k: &str, v: &str| Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header");
    r.with_header(h("Content-Type", ct))
        .with_header(h("Cache-Control", "no-store"))
        .with_header(h("X-Content-Type-Options", "nosniff"))
        .with_header(h("Referrer-Policy", "no-referrer"))
        .with_header(h("Content-Security-Policy", "default-src 'self'; style-src 'self'; script-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'"))
}
