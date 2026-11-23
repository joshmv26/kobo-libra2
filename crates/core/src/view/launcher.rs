//! Home screen with two buttons: Library and Apps. Everything else lives under Apps.
use crate::color::{BLACK, WHITE};
use crate::context::Context;
use crate::font::Fonts;
use crate::framebuffer::{Framebuffer, UpdateMode};
use crate::geom::Rectangle;
use crate::framebuffer::Pixmap;
use crate::metadata::Info;
use std::path::PathBuf;
use chrono::Local;
use crate::gesture::GestureEvent;
use crate::view::{View, Event, Hub, Bus, RenderQueue, RenderData, Id, ID_FEEDER, ViewId, EntryId, AppCmd};
use crate::view::games::{Btn, split_row, draw_text, draw_btn};

#[derive(serde::Deserialize, Clone)]
#[serde(default, rename_all = "kebab-case")]
pub struct HomeConfig {
    pub greeting: String,
    pub background: String,
    pub show_clock: bool,
    pub show_book: bool,
}

impl Default for HomeConfig {
    fn default() -> HomeConfig {
        HomeConfig { greeting: "Hello, Josh".to_string(), background: "home-bg.png".to_string(), show_clock: true, show_book: true }
    }
}

fn load_config() -> HomeConfig {
    std::fs::read_to_string("home.toml").ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn scale_to(pm: &Pixmap, w: u32, h: u32) -> Pixmap {
    let mut out = Pixmap::new(w, h, pm.samples);
    for y in 0..h {
        for x in 0..w {
            let sx = (x as u64 * pm.width as u64 / w as u64) as u32;
            let sy = (y as u64 * pm.height as u64 / h as u64) as u32;
            for k in 0..pm.samples {
                out.data[pm.samples * (y * w + x) as usize + k] = pm.data[pm.samples * (sy * pm.width + sx) as usize + k];
            }
        }
    }
    out
}

#[derive(Clone)]
enum Action {
    Library,
    Apps,
    BackToHome,
    Launch(AppCmd),
    Frontlight,
    Select(EntryId),
}

pub struct Launcher {
    id: Id,
    rect: Rectangle,
    children: Vec<Box<dyn View>>,
    apps_mode: bool,
    buttons: Vec<(Btn, Action)>,
    px: i32,
    book: Option<Info>,
    cover: Option<Pixmap>,
    card: Rectangle,
    cfg: HomeConfig,
    bg: Option<Pixmap>,
}

impl Launcher {
    pub fn new(rect: Rectangle, rq: &mut RenderQueue, _context: &mut Context) -> Launcher {
        let id = ID_FEEDER.next();
        let mut l = Launcher { id, rect, children: Vec::new(), apps_mode: false, buttons: Vec::new(), px: 40,
                               book: None, cover: None, card: rect![0, 0, 0, 0],
                               cfg: load_config(), bg: None };
        l.load_bg();
        l.load_book(_context);
        l.layout();
        rq.add(RenderData::new(id, rect, UpdateMode::Full));
        l
    }

    fn load_bg(&mut self) {
        self.bg = None;
        if self.cfg.background.is_empty() { return; }
        if let Ok(pm) = Pixmap::from_png(&self.cfg.background) {
            self.bg = Some(scale_to(&pm, self.rect.width(), self.rect.height()));
        }
    }

    fn load_book(&mut self, context: &mut Context) {
        let home = context.library.home.clone();
        let (files, _) = context.library.list(&home, None, false);
        let mut best: Option<Info> = None;
        for info in files {
            if let Some(r) = info.reader.as_ref() {
                if r.finished { continue; }
                let newer = match best.as_ref().and_then(|b| b.reader.as_ref()) {
                    Some(br) => r.opened > br.opened,
                    None => true,
                };
                if newer { best = Some(info); }
            }
        }
        if let Some(info) = best.as_ref() {
            let tp: PathBuf = context.library.thumbnail_preview(&info.file.path);
            if let Ok(pm) = Pixmap::from_png(&tp) {
                self.cover = Some(pm);
            }
        }
        self.book = best;
    }

    fn layout(&mut self) {
        let r = self.rect;
        let (w, h) = (r.width() as i32, r.height() as i32);
        let m = w / 14;
        let px = (w / 16).max(28);
        self.px = px;
        self.buttons.clear();
        if !self.apps_mode {
            let top = h / 6;
            let gap = m;
            let card_h = h / 5;
            self.card = rect![r.min.x + m, r.min.y + top, r.max.x - m, r.min.y + top + card_h];
            let by = self.card.max.y + gap;
            let bh = (r.max.y - m - by - gap) / 2;
            let lib = rect![r.min.x + m, by, r.max.x - m, by + bh];
            let apps = rect![r.min.x + m, lib.max.y + gap, r.max.x - m, lib.max.y + gap + bh];
            self.buttons.push((Btn { rect: lib, label: "Library".to_string(), on: false }, Action::Library));
            self.buttons.push((Btn { rect: apps, label: "Apps".to_string(), on: false }, Action::Apps));
        } else {
            let hdr = rect![r.min.x + m, r.min.y + m, r.min.x + m + w / 4, r.min.y + m + h / 14];
            self.buttons.push((Btn { rect: hdr, label: "< Home".to_string(), on: false }, Action::BackToHome));
            let items: Vec<(&str, Action)> = vec![
                ("Sudoku", Action::Launch(AppCmd::Sudoku)),
                ("Tetris", Action::Launch(AppCmd::Tetris)),
                ("Dictionary", Action::Launch(AppCmd::Dictionary { query: String::new(), language: String::new() })),
                ("Calculator", Action::Launch(AppCmd::Calculator)),
                ("Sketch", Action::Launch(AppCmd::Sketch)),
                ("Front light", Action::Frontlight),
                ("Invert colors", Action::Select(EntryId::ToggleInverted)),
                ("WiFi on/off", Action::Select(EntryId::ToggleWifi)),
                ("Screenshot", Action::Select(EntryId::TakeScreenshot)),
                ("About", Action::Select(EntryId::About)),
                ("Reboot", Action::Select(EntryId::Reboot)),
                ("Quit to Kobo", Action::Select(EntryId::Quit)),
            ];
            let rows = (items.len() + 1) / 2;
            let y0 = hdr.max.y + m;
            let gap = m / 2;
            let bh = ((r.max.y - m) - y0 - gap * (rows as i32 - 1)) / rows as i32;
            for (i, (label, action)) in items.into_iter().enumerate() {
                let row = (i / 2) as i32;
                let y = y0 + row * (bh + gap);
                let cols = split_row(r.min.x + m, r.max.x - m, y, y + bh, 2, gap);
                self.buttons.push((Btn { rect: cols[i % 2], label: label.to_string(), on: false }, action));
            }
        }
    }
}

impl View for Launcher {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, _bus: &mut Bus, rq: &mut RenderQueue, _context: &mut Context) -> bool {
        match *evt {
            Event::Gesture(GestureEvent::Tap(pt)) if self.rect.includes(pt) => {
                let hit = self.buttons.iter().find(|(b, _)| b.rect.includes(pt)).map(|(_, a)| a.clone());
                if !self.apps_mode && hit.is_none() && self.cfg.show_book && self.card.includes(pt) {
                    if let Some(info) = self.book.clone() {
                        hub.send(Event::Open(Box::new(info))).ok();
                    }
                    return true;
                }
                match hit {
                    Some(Action::Library) => { hub.send(Event::Back).ok(); },
                    Some(Action::Apps) => {
                        self.apps_mode = true;
                        self.layout();
                        rq.add(RenderData::new(self.id, self.rect, UpdateMode::Full));
                    },
                    Some(Action::BackToHome) => {
                        self.apps_mode = false;
                        self.layout();
                        rq.add(RenderData::new(self.id, self.rect, UpdateMode::Full));
                    },
                    Some(Action::Launch(c)) => { hub.send(Event::Select(EntryId::Launch(c))).ok(); },
                    Some(Action::Frontlight) => { hub.send(Event::Show(ViewId::Frontlight)).ok(); },
                    Some(Action::Select(e)) => { hub.send(Event::Select(e)).ok(); },
                    None => {},
                }
                true
            },
            _ => false,
        }
    }

    fn render(&self, fb: &mut dyn Framebuffer, _rect: Rectangle, fonts: &mut Fonts) {
        fb.draw_rectangle(&self.rect, WHITE);
        if let Some(bg) = self.bg.as_ref() {
            fb.draw_pixmap(bg, self.rect.min);
        }
        let big = if self.apps_mode { self.px } else { self.px * 2 };
        for (b, _) in &self.buttons {
            draw_btn(fb, fonts, b, big);
        }
        if !self.apps_mode {
            let r = self.rect;
            let top = rect![r.min.x, r.min.y, r.max.x, r.min.y + r.height() as i32 / 6];
            let now = Local::now();
            if !self.cfg.show_clock { /* keep greeting only */ }
            let head = rect![top.min.x, top.min.y, top.max.x, top.min.y + top.height() as i32 / 2];
            let sub = rect![top.min.x, head.max.y, top.max.x, top.max.y];
            let strip = |r: &Rectangle, fb: &mut dyn Framebuffer| {
                fb.draw_rectangle(&rect![r.min.x + r.width() as i32 / 8, r.min.y, r.max.x - r.width() as i32 / 8, r.max.y], WHITE);
            };
            if self.bg.is_some() { strip(&head, fb); strip(&sub, fb); }
            if self.cfg.show_clock {
                draw_text(fb, fonts, &now.format("%H:%M").to_string(), self.px * 2, BLACK, &head);
            } else {
                draw_text(fb, fonts, &self.cfg.greeting, self.px * 3 / 2, BLACK, &head);
            }
            let line = if self.cfg.show_clock { format!("{} - {}", self.cfg.greeting, now.format("%A, %-d %B")) } else { now.format("%A, %-d %B").to_string() };
            draw_text(fb, fonts, &line, self.px * 3 / 4, BLACK, &sub);
            if self.cfg.show_book {
            let c = self.card;
            fb.draw_rectangle(&c, BLACK);
            fb.draw_rectangle(&rect![c.min.x + 3, c.min.y + 3, c.max.x - 3, c.max.y - 3], WHITE);
            let pad = (c.height() as i32) / 10;
            let cw = (c.height() as i32 - 2 * pad) * 2 / 3;
            let cover_r = rect![c.min.x + pad, c.min.y + pad, c.min.x + pad + cw, c.max.y - pad];
            fb.draw_rectangle(&cover_r, BLACK);
            fb.draw_rectangle(&rect![cover_r.min.x + 2, cover_r.min.y + 2, cover_r.max.x - 2, cover_r.max.y - 2], WHITE);
            if let Some(pm) = self.cover.as_ref() {
                let (tw, th) = (cover_r.width() as i32 - 4, cover_r.height() as i32 - 4);
                let mut out = Pixmap::new(tw as u32, th as u32, pm.samples);
                for y in 0..th as u32 {
                    for x in 0..tw as u32 {
                        let sx = (x as u64 * pm.width as u64 / tw as u64) as u32;
                        let sy = (y as u64 * pm.height as u64 / th as u64) as u32;
                        for k in 0..pm.samples {
                            out.data[pm.samples * (y * out.width + x) as usize + k] = pm.data[pm.samples * (sy * pm.width + sx) as usize + k];
                        }
                    }
                }
                fb.draw_pixmap(&out, pt!(cover_r.min.x + 2, cover_r.min.y + 2));
            }
            let tx0 = cover_r.max.x + pad;
            let lh = c.height() as i32 / 4;
            let l1 = rect![tx0, c.min.y + pad, c.max.x - pad, c.min.y + pad + lh];
            draw_left(fb, fonts, "Continue reading", self.px * 3 / 4, &l1);
            if let Some(info) = self.book.as_ref() {
                let title: String = info.title.chars().take(34).collect();
                let l2 = rect![tx0, l1.max.y, c.max.x - pad, l1.max.y + lh];
                draw_left(fb, fonts, &title, self.px, &l2);
                let pct = info.reader.as_ref().map(|r| 100 * (r.current_page + 1) / r.pages_count.max(1)).unwrap_or(0);
                let l3 = rect![tx0, l2.max.y, c.max.x - pad, l2.max.y + lh];
                let author: String = info.author.chars().take(26).collect();
                draw_left(fb, fonts, &format!("{} - {}%", author, pct), self.px * 3 / 4, &l3);
                let bar = rect![tx0, l3.max.y + lh / 4, c.max.x - pad, l3.max.y + lh / 4 + lh / 5];
                fb.draw_rectangle(&bar, BLACK);
                fb.draw_rectangle(&rect![bar.min.x + 2, bar.min.y + 2, bar.max.x - 2, bar.max.y - 2], WHITE);
                let fillw = (bar.width() as i32 - 4) * pct as i32 / 100;
                fb.draw_rectangle(&rect![bar.min.x + 2, bar.min.y + 2, bar.min.x + 2 + fillw, bar.max.y - 2], BLACK);
            } else {
                let l2 = rect![tx0, l1.max.y, c.max.x - pad, l1.max.y + lh];
                draw_left(fb, fonts, "No book open yet", self.px, &l2);
            }
            }
        }
    }

    fn resize(&mut self, rect: Rectangle, _hub: &Hub, rq: &mut RenderQueue, _context: &mut Context) {
        self.rect = rect;
        self.layout();
        rq.add(RenderData::new(self.id, rect, UpdateMode::Full));
    }

    fn render_rect(&self, rect: &Rectangle) -> Rectangle {
        rect.intersection(&self.rect).unwrap_or(self.rect)
    }

    fn is_background(&self) -> bool { true }
    fn rect(&self) -> &Rectangle { &self.rect }
    fn rect_mut(&mut self) -> &mut Rectangle { &mut self.rect }
    fn children(&self) -> &Vec<Box<dyn View>> { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn View>> { &mut self.children }
    fn id(&self) -> Id { self.id }
}

fn draw_left(fb: &mut dyn Framebuffer, fonts: &mut Fonts, text: &str, px: i32, rect: &Rectangle) {
    use crate::device::CURRENT_DEVICE;
    use crate::font::{font_from_style, NORMAL_STYLE};
    let dpi = CURRENT_DEVICE.dpi;
    let size = ((px as f32) * 72.0 / dpi as f32 * 64.0).max(64.0) as u32;
    let style = NORMAL_STYLE.with_size(size);
    let font = font_from_style(fonts, &style, dpi);
    let plan = font.plan(text, Some(rect.width() as i32), None);
    let xh = font.x_heights.0 as i32;
    let y = rect.min.y + (rect.height() as i32 + xh * 7 / 5) / 2;
    font.render(fb, BLACK, &plan, pt!(rect.min.x, y));
}
