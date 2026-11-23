//! Telegram helper for Plato. Talks JSON lines over stdin/stdout:
//! one command per input line, one event per output line.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use grammers_client::{Client, SignInError};
use grammers_client::client::LoginToken;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;
use grammers_session::types::DcOption;
use grammers_session::SessionData;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Deserialize)]
struct Config {
    api_id: i32,
    api_hash: String,
    #[serde(default = "default_dl")]
    download_dir: String,
}

fn default_dl() -> String { "/mnt/onboard/Telegram".to_string() }

#[derive(Serialize, Deserialize)]
struct Saved {
    home_dc: i32,
    dc_options: HashMap<i32, DcOption>,
}

const SESSION_FILE: &str = "telegram.session";

fn out(v: Value) {
    println!("{}", v);
}

fn load_session() -> MemorySession {
    let mut data = SessionData::default();
    if let Ok(t) = std::fs::read_to_string(SESSION_FILE) {
        if let Ok(s) = serde_json::from_str::<Saved>(&t) {
            data.home_dc = s.home_dc;
            data.dc_options = s.dc_options;
        }
    }
    MemorySession::from(data)
}

fn save_session(session: &MemorySession) {
    // Export through a throwaway SessionData.
    let tmp = SessionData::default();
    let _ = tmp;
    if let Some(data) = export(session) {
        if let Ok(t) = serde_json::to_string(&data) {
            std::fs::write(SESSION_FILE, t).ok();
        }
    }
}

fn export(session: &MemorySession) -> Option<Saved> {
    use grammers_session::Session;
    let home = session.home_dc_id().ok()?;
    let mut opts = HashMap::new();
    for id in 1..=5 {
        if let Ok(Some(o)) = session.dc_option(id) { opts.insert(id, o); }
    }
    for id in [home] {
        if let Ok(Some(o)) = session.dc_option(id) { opts.insert(id, o); }
    }
    Some(Saved { home_dc: home, dc_options: opts })
}

fn clean_name(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cfg: Config = match std::fs::read_to_string("telegram.toml").ok().and_then(|t| toml::from_str(&t).ok()) {
        Some(c) => c,
        None => {
            out(json!({"ev":"error","msg":"Missing or invalid telegram.toml (needs api_id and api_hash from my.telegram.org)"}));
            return;
        }
    };
    let session = Arc::new(load_session());
    let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), cfg.api_id);
    let client = Client::new(handle);
    tokio::spawn(runner.run());

    let authorized = match client.is_authorized().await {
        Ok(a) => a,
        Err(e) => { out(json!({"ev":"error","msg":format!("Can't reach Telegram (is WiFi on?): {}", e)})); return; }
    };
    out(json!({"ev":"auth","state": if authorized {"ready"} else {"need_phone"}}));
    save_session(&session);

    let mut token: Option<LoginToken> = None;
    let mut pw_token = None;
    let mut dialogs: Vec<grammers_client::peer::Peer> = Vec::new();

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let cmd: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };
        let name = cmd["cmd"].as_str().unwrap_or("");
        match name {
            "phone" => {
                let phone = cmd["phone"].as_str().unwrap_or("").trim().to_string();
                match client.request_login_code(&phone, &cfg.api_hash).await {
                    Ok(t) => { token = Some(t); out(json!({"ev":"code_sent"})); },
                    Err(e) => out(json!({"ev":"error","msg":format!("{}", e)})),
                }
            },
            "code" => {
                let code = cmd["code"].as_str().unwrap_or("").trim().to_string();
                if let Some(t) = token.as_ref() {
                    match client.sign_in(t, &code).await {
                        Ok(_) => { save_session(&session); out(json!({"ev":"auth","state":"ready"})); },
                        Err(SignInError::PasswordRequired(p)) => {
                            out(json!({"ev":"auth","state":"need_password","hint":p.hint().unwrap_or("")}));
                            pw_token = Some(p);
                        },
                        Err(e) => out(json!({"ev":"error","msg":format!("{}", e)})),
                    }
                }
            },
            "password" => {
                let pw = cmd["password"].as_str().unwrap_or("").to_string();
                if let Some(p) = pw_token.take() {
                    match client.check_password(p, pw.trim()).await {
                        Ok(_) => { save_session(&session); out(json!({"ev":"auth","state":"ready"})); },
                        Err(e) => out(json!({"ev":"error","msg":format!("{}", e)})),
                    }
                }
            },
            "dialogs" => {
                dialogs.clear();
                let mut list = Vec::new();
                let mut it = client.iter_dialogs();
                while let Ok(Some(d)) = it.next().await {
                    let peer = d.peer().clone();
                    list.push(json!({"i": dialogs.len(), "name": peer.name().unwrap_or("?")}));
                    dialogs.push(peer);
                    if dialogs.len() >= 30 { break; }
                }
                save_session(&session);
                out(json!({"ev":"dialogs","list":list}));
            },
            "messages" => {
                let i = cmd["peer"].as_u64().unwrap_or(0) as usize;
                let limit = cmd["limit"].as_u64().unwrap_or(15) as usize;
                if let Some(peer) = dialogs.get(i) {
                    match peer.to_ref().await {
                        Ok(Some(pr)) => {
                            let mut it = client.iter_messages(pr).limit(limit);
                            let mut msgs = Vec::new();
                            while let Ok(Some(m)) = it.next().await {
                                let file = m.media().map(|md| match md {
                                    grammers_client::media::Media::Photo(_) => "photo".to_string(),
                                    grammers_client::media::Media::Document(d) => d.name().map(|s| s.to_string()).unwrap_or("file".to_string()),
                                    _ => "attachment".to_string(),
                                });
                                msgs.push(json!({"id": m.id(), "out": m.outgoing(), "text": m.text(), "file": file}));
                            }
                            msgs.reverse();
                            out(json!({"ev":"messages","peer":i,"list":msgs}));
                        },
                        _ => out(json!({"ev":"error","msg":"chat not available"})),
                    }
                }
            },
            "send" => {
                let i = cmd["peer"].as_u64().unwrap_or(0) as usize;
                let text = cmd["text"].as_str().unwrap_or("").to_string();
                if let Some(peer) = dialogs.get(i) {
                    if let Ok(Some(pr)) = peer.to_ref().await {
                        match client.send_message(pr, text).await {
                            Ok(_) => out(json!({"ev":"sent","peer":i})),
                            Err(e) => out(json!({"ev":"error","msg":format!("{}", e)})),
                        }
                    }
                }
            },
            "download" => {
                let i = cmd["peer"].as_u64().unwrap_or(0) as usize;
                let id = cmd["msg"].as_i64().unwrap_or(0) as i32;
                if let Some(peer) = dialogs.get(i) {
                    if let Ok(Some(pr)) = peer.to_ref().await {
                        let mut it = client.iter_messages(pr).offset_id(id + 1).limit(1);
                        match it.next().await {
                            Ok(Some(m)) if m.id() == id => {
                                if let Some(media) = m.media() {
                                    let fname = match &media {
                                        grammers_client::media::Media::Document(d) => d.name().map(clean_name).unwrap_or_else(|| format!("file-{}", id)),
                                        grammers_client::media::Media::Photo(_) => format!("photo-{}.jpg", id),
                                        _ => format!("attachment-{}", id),
                                    };
                                    let mut path = PathBuf::from(&cfg.download_dir);
                                    std::fs::create_dir_all(&path).ok();
                                    path.push(fname);
                                    match client.download_media(&media, &path).await {
                                        Ok(_) => out(json!({"ev":"downloaded","path":path.to_string_lossy()})),
                                        Err(e) => out(json!({"ev":"error","msg":format!("{}", e)})),
                                    }
                                } else {
                                    out(json!({"ev":"error","msg":"no file in that message"}));
                                }
                            },
                            _ => out(json!({"ev":"error","msg":"message not found"})),
                        }
                    }
                }
            },
            "quit" => break,
            _ => {},
        }
    }
}
