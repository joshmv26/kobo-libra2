//! Home screen with two buttons: Library and Apps. Everything else lives under Apps.
use crate::color::{BLACK, WHITE};
use crate::context::Context;
use crate::font::Fonts;
use crate::framebuffer::{Framebuffer, UpdateMode};
use crate::geom::Rectangle;
use crate::gesture::GestureEvent;
use crate::view::{View, Event, Hub, Bus, RenderQueue, RenderData, Id, ID_FEEDER, ViewId, EntryId, AppCmd};
use crate::view::games::{Btn, split_row, draw_text, draw_btn};

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
}

impl Launcher {
    pub fn new(rect: Rectangle, rq: &mut RenderQueue, _context: &mut Context) -> Launcher {
        let id = ID_FEEDER.next();
        let mut l = Launcher { id, rect, children: Vec::new(), apps_mode: false, buttons: Vec::new(), px: 40 };
        l.layout();
        rq.add(RenderData::new(id, rect, UpdateMode::Full));
        l
    }

    fn layout(&mut self) {
        let r = self.rect;
        let (w, h) = (r.width() as i32, r.height() as i32);
        let m = w / 14;
        let px = (w / 16).max(28);
        self.px = px;
        self.buttons.clear();
        if !self.apps_mode {
            let top = h / 5;
            let gap = m;
            let bh = (h - top - m * 2 - gap) / 2;
            let lib = rect![r.min.x + m, r.min.y + top, r.max.x - m, r.min.y + top + bh];
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
        let big = if self.apps_mode { self.px } else { self.px * 2 };
        for (b, _) in &self.buttons {
            draw_btn(fb, fonts, b, big);
        }
        if !self.apps_mode {
            let title = rect![self.rect.min.x, self.rect.min.y, self.rect.max.x, self.rect.min.y + self.rect.height() as i32 / 5];
            draw_text(fb, fonts, "Home", self.px * 2, BLACK, &title);
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
