//! Telegram client screens. A helper program (plato-tg) speaks to Telegram;
//! this view talks to it with JSON lines over stdin/stdout.
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use serde_json::{json, Value};
use crate::color::{BLACK, WHITE};
use crate::context::Context;
use crate::device::CURRENT_DEVICE;
use crate::font::{Fonts, font_from_style, NORMAL_STYLE};
use crate::framebuffer::{Framebuffer, UpdateMode};
use crate::geom::Rectangle;
use crate::gesture::GestureEvent;
use crate::view::keyboard::Keyboard;
use crate::view::{View, Event, Hub, Bus, RenderQueue, RenderData, Id, ID_FEEDER, KeyboardEvent, TextKind};
use crate::view::games::{Btn, split_row, draw_text, draw_btn};
use crate::geom::LinearDir;

#[derive(Clone, PartialEq)]
enum Screen { Status, Phone, Code, Password, Chats, Chat, Files, Reply }

#[derive(Clone)]
enum Act { Back, Refresh, Open(usize), Next, Prev, Reply, Files, Get(i64), ChatBack }

struct Msg { id: i64, out: bool, text: String, file: Option<String> }

pub struct Telegram {
    id: Id,
    rect: Rectangle,
    children: Vec<Box<dyn View>>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    screen: Screen,
    status: String,
    chats: Vec<String>,
    page: usize,
    cur: usize,
    msgs: Vec<Msg>,
    input: String,
    buttons: Vec<(Btn, Act)>,
    px: i32,
    kb_top: i32,
}

fn fits(fonts: &mut Fonts, text: &str, px: i32, width: i32) -> bool {
    let dpi = CURRENT_DEVICE.dpi;
    let size = ((px as f32) * 72.0 / dpi as f32 * 64.0).max(64.0) as u32;
    let style = NORMAL_STYLE.with_size(size);
    let font = font_from_style(fonts, &style, dpi);
    font.plan(text, None, None).width <= width
}

fn wrap(fonts: &mut Fonts, text: &str, px: i32, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{} {}", cur, word) };
            if fits(fonts, &cand, px, width) { cur = cand; }
            else {
                if !cur.is_empty() { lines.push(cur); }
                cur = word.to_string();
            }
        }
        lines.push(cur);
    }
    lines
}

fn draw_left(fb: &mut dyn Framebuffer, fonts: &mut Fonts, text: &str, px: i32, rect: &Rectangle) {
    let dpi = CURRENT_DEVICE.dpi;
    let size = ((px as f32) * 72.0 / dpi as f32 * 64.0).max(64.0) as u32;
    let style = NORMAL_STYLE.with_size(size);
    let font = font_from_style(fonts, &style, dpi);
    let plan = font.plan(text, Some(rect.width() as i32), None);
    let xh = font.x_heights.0 as i32;
    let y = rect.min.y + (rect.height() as i32 + xh * 7 / 5) / 2;
    font.render(fb, BLACK, &plan, pt!(rect.min.x, y));
}

impl Telegram {
    pub fn new(rect: Rectangle, hub: &Hub, rq: &mut RenderQueue, _context: &mut Context) -> Telegram {
        let id = ID_FEEDER.next();
        let mut t = Telegram {
            id, rect, children: Vec::new(), child: None, stdin: None, screen: Screen::Status,
            status: "Connecting to Telegram...".to_string(), chats: Vec::new(), page: 0, cur: 0,
            msgs: Vec::new(), input: String::new(), buttons: Vec::new(), px: 40, kb_top: rect.max.y,
        };
        t.start(hub);
        t.layout();
        rq.add(RenderData::new(id, rect, UpdateMode::Full));
        t
    }

    fn start(&mut self, hub: &Hub) {
        match Command::new("./plato-tg").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Ok(mut child) => {
                self.stdin = child.stdin.take();
                let out = child.stdout.take();
                self.child = Some(child);
                let hub2 = hub.clone();
                if let Some(out) = out {
                    thread::spawn(move || {
                        for line in BufReader::new(out).lines() {
                            match line { Ok(l) => { hub2.send(Event::TgLine(l)).ok(); }, Err(_) => break }
                        }
                        hub2.send(Event::TgLine("{\"ev\":\"closed\"}".to_string())).ok();
                    });
                }
            },
            Err(e) => self.status = format!("Can't start the Telegram helper: {}", e),
        }
    }

    fn send(&mut self, v: Value) {
        if let Some(s) = self.stdin.as_mut() {
            writeln!(s, "{}", v).ok();
            s.flush().ok();
        }
    }

    fn typing(&self) -> bool { matches!(self.screen, Screen::Phone | Screen::Code | Screen::Password | Screen::Reply) }

    fn layout(&mut self) {
        let r = self.rect;
        let (w, h) = (r.width() as i32, r.height() as i32);
        let m = w / 28;
        let px = (w / 24).max(24);
        self.px = px;
        self.buttons.clear();
        let bar_h = h / 14;
        let back = rect![r.min.x + m, r.min.y + m, r.min.x + m + w / 4, r.min.y + m + bar_h];
        match self.screen {
            Screen::Status | Screen::Phone | Screen::Code | Screen::Password => {
                self.buttons.push((Btn { rect: back, label: "< Back".to_string(), on: false }, Act::Back));
            },
            Screen::Reply => {
                self.buttons.push((Btn { rect: back, label: "Cancel".to_string(), on: false }, Act::ChatBack));
            },
            Screen::Chats => {
                self.buttons.push((Btn { rect: back, label: "< Back".to_string(), on: false }, Act::Back));
                let rf = rect![r.max.x - m - w / 4, back.min.y, r.max.x - m, back.max.y];
                self.buttons.push((Btn { rect: rf, label: "Refresh".to_string(), on: false }, Act::Refresh));
                let per = 8usize;
                let y0 = back.max.y + m;
                let nav_h = bar_h;
                let rows_h = (r.max.y - m - nav_h - m) - y0;
                let rh = rows_h / per as i32;
                for (n, i) in (self.page * per..((self.page + 1) * per).min(self.chats.len())).enumerate() {
                    let y = y0 + n as i32 * rh;
                    let rr = rect![r.min.x + m, y, r.max.x - m, y + rh - m / 2];
                    self.buttons.push((Btn { rect: rr, label: self.chats[i].chars().take(34).collect(), on: false }, Act::Open(i)));
                }
                let ny = r.max.y - m - nav_h;
                let cols = split_row(r.min.x + m, r.max.x - m, ny, ny + nav_h, 2, m);
                self.buttons.push((Btn { rect: cols[0], label: "< Prev".to_string(), on: false }, Act::Prev));
                self.buttons.push((Btn { rect: cols[1], label: "Next >".to_string(), on: false }, Act::Next));
            },
            Screen::Chat => {
                let by = r.max.y - m - bar_h;
                let cols = split_row(r.min.x + m, r.max.x - m, by, by + bar_h, 4, m / 2);
                self.buttons.push((Btn { rect: cols[0], label: "< Chats".to_string(), on: false }, Act::ChatBack));
                self.buttons.push((Btn { rect: cols[1], label: "Refresh".to_string(), on: false }, Act::Refresh));
                self.buttons.push((Btn { rect: cols[2], label: "Reply".to_string(), on: false }, Act::Reply));
                self.buttons.push((Btn { rect: cols[3], label: "Files".to_string(), on: false }, Act::Files));
            },
            Screen::Files => {
                self.buttons.push((Btn { rect: back, label: "< Chat".to_string(), on: false }, Act::Refresh));
                let files: Vec<&Msg> = self.msgs.iter().filter(|m| m.file.is_some()).collect();
                let y0 = back.max.y + m;
                let rh = bar_h * 3 / 2;
                for (n, f) in files.iter().rev().take(8).enumerate() {
                    let y = y0 + n as i32 * (rh + m / 2);
                    let rr = rect![r.min.x + m, y, r.max.x - m, y + rh];
                    let label: String = format!("Get: {}", f.file.clone().unwrap_or_default()).chars().take(34).collect();
                    self.buttons.push((Btn { rect: rr, label, on: false }, Act::Get(f.id)));
                }
            },
        }
    }

    fn open_keyboard(&mut self, hub: &Hub, rq: &mut RenderQueue, context: &mut Context) {
        if !self.children.is_empty() { return; }
        let h = self.rect.height() as i32;
        let mut kb_rect = rect![self.rect.min.x, self.rect.max.y - h * 2 / 5, self.rect.max.x, self.rect.max.y];
        let kb = Keyboard::new(&mut kb_rect, self.screen == Screen::Code || self.screen == Screen::Phone, context);
        self.kb_top = kb_rect.min.y;
        context.kb_rect = kb_rect;
        rq.add(RenderData::new(kb.id(), kb_rect, UpdateMode::Gui));
        self.children.push(Box::new(kb) as Box<dyn View>);
        let _ = hub;
    }

    fn close_keyboard(&mut self, rq: &mut RenderQueue, context: &mut Context) {
        if let Some(c) = self.children.pop() {
            rq.add(RenderData::expose(*c.rect(), UpdateMode::Gui));
        }
        context.kb_rect = Rectangle::default();
        self.kb_top = self.rect.max.y;
    }

    fn go(&mut self, screen: Screen, hub: &Hub, rq: &mut RenderQueue, context: &mut Context) {
        let was_typing = self.typing();
        self.screen = screen;
        self.input.clear();
        if self.typing() { self.open_keyboard(hub, rq, context); } else if was_typing { self.close_keyboard(rq, context); }
        self.layout();
        rq.add(RenderData::new(self.id, self.rect, UpdateMode::Full));
    }

    fn handle_line(&mut self, line: &str, hub: &Hub, rq: &mut RenderQueue, context: &mut Context) {
        let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => return };
        match v["ev"].as_str().unwrap_or("") {
            "auth" => match v["state"].as_str().unwrap_or("") {
                "need_phone" => { self.status = "Enter your phone number (with +34 etc.), then press Enter on the keyboard.".to_string(); self.go(Screen::Phone, hub, rq, context); },
                "need_password" => { self.status = format!("Two-step password (hint: {})", v["hint"].as_str().unwrap_or("")); self.go(Screen::Password, hub, rq, context); },
                "ready" => { self.status = "Loading chats...".to_string(); self.go(Screen::Status, hub, rq, context); self.send(json!({"cmd":"dialogs"})); },
                _ => {},
            },
            "code_sent" => { self.status = "Enter the code Telegram sent you (in the Telegram app or SMS).".to_string(); self.go(Screen::Code, hub, rq, context); },
            "dialogs" => {
                self.chats = v["list"].as_array().map(|a| a.iter().map(|d| d["name"].as_str().unwrap_or("?").to_string()).collect()).unwrap_or_default();
                self.page = 0;
                self.go(Screen::Chats, hub, rq, context);
            },
            "messages" => {
                self.msgs = v["list"].as_array().map(|a| a.iter().map(|m| Msg {
                    id: m["id"].as_i64().unwrap_or(0), out: m["out"].as_bool().unwrap_or(false),
                    text: m["text"].as_str().unwrap_or("").to_string(), file: m["file"].as_str().map(|s| s.to_string()),
                }).collect()).unwrap_or_default();
                self.go(Screen::Chat, hub, rq, context);
            },
            "sent" => { self.send(json!({"cmd":"messages","peer":self.cur,"limit":12})); },
            "downloaded" => {
                self.status = format!("Saved to {}", v["path"].as_str().unwrap_or("Telegram folder"));
                self.go(Screen::Status, hub, rq, context);
            },
            "error" => { self.status = format!("Error: {}", v["msg"].as_str().unwrap_or("")); self.go(Screen::Status, hub, rq, context); },
            "closed" => { if self.screen == Screen::Status && self.status.starts_with("Connecting") { self.status = "The Telegram helper stopped. Check telegram.toml (api_id, api_hash) and WiFi.".to_string(); self.go(Screen::Status, hub, rq, context); } },
            _ => {},
        }
    }

    fn submit(&mut self, hub: &Hub, rq: &mut RenderQueue, context: &mut Context) {
        let text = self.input.trim().to_string();
        if text.is_empty() { return; }
        match self.screen {
            Screen::Phone => { self.send(json!({"cmd":"phone","phone":text})); self.status = "Requesting code...".to_string(); },
            Screen::Code => { self.send(json!({"cmd":"code","code":text})); self.status = "Signing in...".to_string(); },
            Screen::Password => { self.send(json!({"cmd":"password","password":self.input.clone()})); self.status = "Signing in...".to_string(); },
            Screen::Reply => { self.send(json!({"cmd":"send","peer":self.cur,"text":text})); self.status = "Sending...".to_string(); },
            _ => return,
        }
        self.go(Screen::Status, hub, rq, context);
    }
}

impl Drop for Telegram {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() { c.kill().ok(); c.wait().ok(); }
    }
}

impl View for Telegram {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, _bus: &mut Bus, rq: &mut RenderQueue, context: &mut Context) -> bool {
        match *evt {
            Event::TgLine(ref line) => { let l = line.clone(); self.handle_line(&l, hub, rq, context); true },
            Event::Keyboard(kb) if self.typing() => {
                match kb {
                    KeyboardEvent::Append(c) => self.input.push(c),
                    KeyboardEvent::Delete { target: TextKind::Char, dir: LinearDir::Backward } => { self.input.pop(); },
                    KeyboardEvent::Delete { .. } => { self.input.clear(); },
                    KeyboardEvent::Submit => { self.submit(hub, rq, context); return true; },
                    _ => return true,
                }
                rq.add(RenderData::new(self.id, rect![self.rect.min.x, self.rect.min.y, self.rect.max.x, self.kb_top], UpdateMode::Fast));
                true
            },
            Event::Gesture(GestureEvent::Tap(pt)) if self.rect.includes(pt) => {
                let hit = self.buttons.iter().find(|(b, _)| b.rect.includes(pt)).map(|(_, a)| a.clone());
                match hit {
                    Some(Act::Back) => { hub.send(Event::Back).ok(); },
                    Some(Act::Refresh) => {
                        if self.screen == Screen::Chats { self.send(json!({"cmd":"dialogs"})); }
                        else { self.send(json!({"cmd":"messages","peer":self.cur,"limit":12})); }
                        self.status = "Loading...".to_string();
                        self.go(Screen::Status, hub, rq, context);
                    },
                    Some(Act::Open(i)) => { self.cur = i; self.send(json!({"cmd":"messages","peer":i,"limit":12})); self.status = "Loading messages...".to_string(); self.go(Screen::Status, hub, rq, context); },
                    Some(Act::Next) => { if (self.page + 1) * 8 < self.chats.len() { self.page += 1; self.layout(); rq.add(RenderData::new(self.id, self.rect, UpdateMode::Gui)); } },
                    Some(Act::Prev) => { if self.page > 0 { self.page -= 1; self.layout(); rq.add(RenderData::new(self.id, self.rect, UpdateMode::Gui)); } },
                    Some(Act::Reply) => self.go(Screen::Reply, hub, rq, context),
                    Some(Act::Files) => self.go(Screen::Files, hub, rq, context),
                    Some(Act::Get(id)) => { self.send(json!({"cmd":"download","peer":self.cur,"msg":id})); self.status = "Downloading...".to_string(); self.go(Screen::Status, hub, rq, context); },
                    Some(Act::ChatBack) => {
                        if self.screen == Screen::Reply { self.go(Screen::Chat, hub, rq, context); }
                        else { self.go(Screen::Chats, hub, rq, context); }
                    },
                    None => {
                        if self.screen == Screen::Status && !self.status.starts_with("Connecting") && !self.status.starts_with("Loading") && !self.status.starts_with("Down") {
                            // Tap anywhere on a result message returns to the chat list.
                            if !self.chats.is_empty() { self.go(Screen::Chats, hub, rq, context); }
                        }
                    },
                }
                true
            },
            _ => false,
        }
    }

    fn render(&self, fb: &mut dyn Framebuffer, _rect: Rectangle, fonts: &mut Fonts) {
        let r = self.rect;
        fb.draw_rectangle(&r, WHITE);
        for (b, _) in &self.buttons { draw_btn(fb, fonts, b, self.px); }
        let m = r.width() as i32 / 28;
        let px = self.px;
        let lh = px * 3 / 2;
        let top = self.buttons.first().map(|(b, _)| b.rect.max.y + m).unwrap_or(r.min.y + m);
        let width = r.width() as i32 - 2 * m;
        match self.screen {
            Screen::Status => {
                let mut y = top + m;
                for l in wrap(fonts, &self.status, px, width) {
                    draw_left(fb, fonts, &l, px, &rect![r.min.x + m, y, r.max.x - m, y + lh]);
                    y += lh;
                }
            },
            Screen::Phone | Screen::Code | Screen::Password | Screen::Reply => {
                let mut y = top + m;
                if self.screen == Screen::Reply {
                    let name = self.chats.get(self.cur).cloned().unwrap_or_default();
                    draw_left(fb, fonts, &format!("Reply to {}", name), px, &rect![r.min.x + m, y, r.max.x - m, y + lh]);
                    y += lh;
                } else {
                    for l in wrap(fonts, &self.status, px, width) {
                        draw_left(fb, fonts, &l, px, &rect![r.min.x + m, y, r.max.x - m, y + lh]);
                        y += lh;
                    }
                }
                y += m;
                let shown = if self.screen == Screen::Password { "*".repeat(self.input.chars().count()) } else { self.input.clone() };
                let box_r = rect![r.min.x + m, y, r.max.x - m, y + lh * 5 / 2];
                fb.draw_rectangle(&box_r, BLACK);
                fb.draw_rectangle(&rect![box_r.min.x + 2, box_r.min.y + 2, box_r.max.x - 2, box_r.max.y - 2], WHITE);
                let lines = wrap(fonts, &format!("{}_", shown), px, width - 20);
                let start = lines.len().saturating_sub(2);
                for (n, l) in lines[start..].iter().enumerate() {
                    let yy = box_r.min.y + n as i32 * lh + 4;
                    draw_left(fb, fonts, l, px, &rect![box_r.min.x + 10, yy, box_r.max.x - 10, yy + lh]);
                }
            },
            Screen::Chats => {
                let t = rect![r.min.x + r.width() as i32 / 4, r.min.y + m, r.max.x - r.width() as i32 / 4 - m, r.min.y + m + r.height() as i32 / 14];
                draw_text(fb, fonts, "Telegram", px, BLACK, &t);
            },
            Screen::Chat | Screen::Files => {
                let name = self.chats.get(self.cur).cloned().unwrap_or_default();
                let t = rect![r.min.x + m, r.min.y + m, r.max.x - m, r.min.y + m + r.height() as i32 / 16];
                if self.screen == Screen::Chat { draw_text(fb, fonts, &name, px, BLACK, &t); }
                if self.screen == Screen::Chat {
                    // Newest messages at the bottom.
                    let bottom = self.buttons.first().map(|(b, _)| b.rect.min.y - m).unwrap_or(r.max.y);
                    let mut lines: Vec<(String, bool)> = Vec::new();
                    for msg in &self.msgs {
                        let mut text = msg.text.clone();
                        if let Some(f) = msg.file.as_ref() { text = format!("[file: {}] {}", f, text); }
                        let prefix = if msg.out { "Me: " } else { "" };
                        for (i, l) in wrap(fonts, &format!("{}{}", prefix, text), px * 4 / 5, width).into_iter().enumerate() {
                            lines.push((if i == 0 { l } else { format!("   {}", l) }, msg.out));
                        }
                        lines.push((String::new(), false));
                    }
                    let lh2 = px;
                    let top2 = t.max.y + m;
                    let cap = ((bottom - top2) / lh2).max(1) as usize;
                    let start = lines.len().saturating_sub(cap);
                    for (n, (l, _)) in lines[start..].iter().enumerate() {
                        let yy = top2 + n as i32 * lh2;
                        draw_left(fb, fonts, l, px * 4 / 5, &rect![r.min.x + m, yy, r.max.x - m, yy + lh2]);
                    }
                }
            },
        }
    }

    fn resize(&mut self, rect: Rectangle, _hub: &Hub, rq: &mut RenderQueue, _context: &mut Context) {
        self.rect = rect;
        self.layout();
        rq.add(RenderData::new(self.id, rect, UpdateMode::Full));
    }

    fn render_rect(&self, rect: &Rectangle) -> Rectangle { rect.intersection(&self.rect).unwrap_or(self.rect) }
    fn is_background(&self) -> bool { true }
    fn rect(&self) -> &Rectangle { &self.rect }
    fn rect_mut(&mut self) -> &mut Rectangle { &mut self.rect }
    fn children(&self) -> &Vec<Box<dyn View>> { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn View>> { &mut self.children }
    fn id(&self) -> Id { self.id }
}
