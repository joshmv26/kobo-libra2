use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use crate::color::{Color, BLACK, WHITE, GRAY09, GRAY13};
use crate::context::Context;
use crate::font::Fonts;
use crate::framebuffer::{Framebuffer, UpdateMode};
use crate::geom::{BorderSpec, Rectangle, Point};
use crate::gesture::GestureEvent;
use crate::view::{View, Event, Hub, Bus, RenderQueue, RenderData, Id, ID_FEEDER};
use super::{Btn, Rng, split_row, draw_text, draw_btn, LINE};

const W: usize = 10;
const H: usize = 20;
const SHAPES: [&[&[u8]]; 7] = [
    &[&[1, 1, 1, 1]],
    &[&[1, 1], &[1, 1]],
    &[&[0, 1, 0], &[1, 1, 1]],
    &[&[0, 1, 1], &[1, 1, 0]],
    &[&[1, 1, 0], &[0, 1, 1]],
    &[&[1, 0, 0], &[1, 1, 1]],
    &[&[0, 0, 1], &[1, 1, 1]],
];
const GRAYS: [u8; 7] = [70, 120, 160, 95, 135, 45, 175];

type Mat = Vec<Vec<u8>>;

fn shape(k: usize) -> Mat {
    SHAPES[k].iter().map(|r| r.to_vec()).collect()
}

fn rotate(m: &Mat) -> Mat {
    let h = m.len();
    let w = m[0].len();
    let mut out = vec![vec![0u8; h]; w];
    for r in 0..h {
        for c in 0..w {
            out[c][h - 1 - r] = m[r][c];
        }
    }
    out
}

struct Piece {
    kind: usize,
    m: Mat,
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct Layout {
    back: Rectangle,
    pause: Rectangle,
    new_game: Rectangle,
    board: Rectangle,
    cell: i32,
    panel: Rectangle,
    left: Rectangle,
    rot: Rectangle,
    right: Rectangle,
    down: Rectangle,
    drop: Rectangle,
    px: i32,
}

fn layout(rect: Rectangle) -> Layout {
    let w = rect.width() as i32;
    let m = (w / 28).max(12);
    let gap = (m / 2).max(6);
    let btn = (w / 11).max(48);
    let big = btn * 3 / 2;
    let (x0, x1) = (rect.min.x + m, rect.max.x - m);
    let mut y = rect.min.y + m;
    let top = split_row(x0, x1, y, y + btn, 3, gap);
    y += btn + gap;
    let below = big * 2 + gap * 2 + m;
    let avail_h = rect.max.y - y - below;
    let cell = (avail_h / H as i32).min(((x1 - x0) * 6 / 10) / W as i32).max(12);
    let board = rect![x0, y, x0 + cell * W as i32, y + cell * H as i32];
    let panel = rect![board.max.x + gap * 2, y, x1, board.max.y];
    let yb = board.max.y + gap;
    let r1 = split_row(x0, x1, yb, yb + big, 3, gap);
    let r2 = split_row(x0, x1, yb + big + gap, yb + big * 2 + gap, 2, gap);
    Layout {
        back: top[0],
        pause: top[1],
        new_game: top[2],
        board,
        cell,
        panel,
        left: r1[0],
        rot: r1[1],
        right: r1[2],
        down: r2[0],
        drop: r2[1],
        px: (btn * 2 / 5).max(14),
    }
}

pub struct Tetris {
    id: Id,
    rect: Rectangle,
    children: Vec<Box<dyn View>>,
    lay: Layout,
    board: [[u8; W]; H],
    piece: Piece,
    next: usize,
    score: u32,
    lines: u32,
    level: u32,
    best: u32,
    over: bool,
    paused: bool,
    ticks: u32,
    rng: Rng,
    running: Arc<AtomicBool>,
    interval: Arc<AtomicU64>,
}

fn speed_ms(level: u32) -> u64 {
    // Slow on purpose: e-ink needs time to refresh.
    (1100u64).saturating_sub(level as u64 * 80).max(700)
}

impl Tetris {
    pub fn new(rect: Rectangle, hub: &Hub, rq: &mut RenderQueue, _context: &mut Context) -> Tetris {
        let id = ID_FEEDER.next();
        let mut rng = Rng::new();
        let first = rng.below(7);
        let next = rng.below(7);
        let running = Arc::new(AtomicBool::new(true));
        let interval = Arc::new(AtomicU64::new(speed_ms(0)));
        {
            let hub2 = hub.clone();
            let running2 = running.clone();
            let interval2 = interval.clone();
            thread::spawn(move || {
                while running2.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(interval2.load(Ordering::Relaxed)));
                    if !running2.load(Ordering::Relaxed) || hub2.send(Event::GameTick).is_err() {
                        break;
                    }
                }
            });
        }
        let m = shape(first);
        let x = ((W - m[0].len()) / 2) as i32;
        let t = Tetris {
            id,
            rect,
            children: Vec::new(),
            lay: layout(rect),
            board: [[0; W]; H],
            piece: Piece { kind: first, m, x, y: 0 },
            next,
            score: 0,
            lines: 0,
            level: 0,
            best: 0,
            over: false,
            paused: false,
            ticks: 0,
            rng,
            running,
            interval,
        };
        rq.add(RenderData::new(id, rect, UpdateMode::Full));
        t
    }

    fn hit(&self, m: &Mat, x: i32, y: i32) -> bool {
        for (r, row) in m.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                if v != 0 {
                    let (xx, yy) = (x + c as i32, y + r as i32);
                    if xx < 0 || xx >= W as i32 || yy >= H as i32 {
                        return true;
                    }
                    if yy >= 0 && self.board[yy as usize][xx as usize] != 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn spawn(&mut self) {
        let kind = self.next;
        self.next = self.rng.below(7);
        let m = shape(kind);
        let x = ((W - m[0].len()) / 2) as i32;
        self.piece = Piece { kind, m, x, y: 0 };
        if self.hit(&self.piece.m, self.piece.x, self.piece.y) {
            self.over = true;
            self.best = self.best.max(self.score);
        }
    }

    fn lock(&mut self) {
        let (kind, x, y) = (self.piece.kind, self.piece.x, self.piece.y);
        for (r, row) in self.piece.m.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                if v != 0 {
                    let (xx, yy) = (x + c as i32, y + r as i32);
                    if yy >= 0 {
                        self.board[yy as usize][xx as usize] = kind as u8 + 1;
                    }
                }
            }
        }
        let mut cleared = 0u32;
        let mut r = H as i32 - 1;
        while r >= 0 {
            if self.board[r as usize].iter().all(|&v| v != 0) {
                for k in (1..=r as usize).rev() {
                    self.board[k] = self.board[k - 1];
                }
                self.board[0] = [0; W];
                cleared += 1;
            } else {
                r -= 1;
            }
        }
        if cleared > 0 {
            self.lines += cleared;
            self.score += [0, 40, 100, 300, 1200][cleared.min(4) as usize] * (self.level + 1);
            let lv = self.lines / 10;
            if lv != self.level {
                self.level = lv;
                self.interval.store(speed_ms(lv), Ordering::Relaxed);
            }
        }
        self.score += 2;
        self.spawn();
    }

    fn step_down(&mut self, manual: bool) {
        let (x, y) = (self.piece.x, self.piece.y);
        if !self.hit(&self.piece.m, x, y + 1) {
            self.piece.y += 1;
            if manual { self.score += 1; }
        } else {
            self.lock();
        }
    }

    fn act(&mut self, a: &str) {
        if self.over || self.paused { return; }
        let (x, y) = (self.piece.x, self.piece.y);
        match a {
            "left" => { if !self.hit(&self.piece.m, x - 1, y) { self.piece.x -= 1; } },
            "right" => { if !self.hit(&self.piece.m, x + 1, y) { self.piece.x += 1; } },
            "rot" => {
                let r = rotate(&self.piece.m);
                for k in [0, -1, 1, -2, 2] {
                    if !self.hit(&r, x + k, y) {
                        self.piece.m = r;
                        self.piece.x = x + k;
                        break;
                    }
                }
            },
            "down" => self.step_down(true),
            "drop" => {
                while !self.hit(&self.piece.m, self.piece.x, self.piece.y + 1) {
                    self.piece.y += 1;
                    self.score += 2;
                }
                self.lock();
            },
            _ => {},
        }
    }

    fn restart(&mut self) {
        self.board = [[0; W]; H];
        self.best = self.best.max(self.score);
        self.score = 0;
        self.lines = 0;
        self.level = 0;
        self.over = false;
        self.paused = false;
        self.interval.store(speed_ms(0), Ordering::Relaxed);
        self.next = self.rng.below(7);
        self.spawn();
    }

    fn play_rect(&self) -> Rectangle {
        rect![self.lay.board.min.x, self.lay.board.min.y, self.lay.panel.max.x, self.lay.board.max.y]
    }

    fn tap(&mut self, pt: Point, hub: &Hub) -> bool {
        let lay = self.lay.clone();
        if lay.back.includes(pt) { hub.send(Event::Back).ok(); return true; }
        if lay.pause.includes(pt) { if !self.over { self.paused = !self.paused; } return true; }
        if lay.new_game.includes(pt) { self.restart(); return true; }
        if self.paused && !self.over {
            self.paused = false; // any control resumes
        }
        if lay.left.includes(pt) { self.act("left"); return true; }
        if lay.right.includes(pt) { self.act("right"); return true; }
        if lay.rot.includes(pt) { self.act("rot"); return true; }
        if lay.down.includes(pt) { self.act("down"); return true; }
        if lay.drop.includes(pt) { self.act("drop"); return true; }
        if lay.board.includes(pt) {
            let f = (pt.x - lay.board.min.x) as f32 / lay.board.width() as f32;
            self.act(if f < 0.33 { "left" } else if f > 0.66 { "right" } else { "rot" });
            return true;
        }
        false
    }

    fn draw_cell(&self, fb: &mut dyn Framebuffer, x: i32, y: i32, size: i32, kind: usize) {
        let r = rect![x, y, x + size, y + size];
        fb.draw_rectangle(&r, BLACK);
        let inner = rect![x + 2, y + 2, x + size - 2, y + size - 2];
        fb.draw_rectangle(&inner, Color::Gray(GRAYS[kind]));
    }
}

impl Drop for Tetris {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl View for Tetris {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, _bus: &mut Bus, rq: &mut RenderQueue, _context: &mut Context) -> bool {
        match *evt {
            Event::GameTick => {
                if !self.paused && !self.over {
                    self.step_down(false);
                    self.ticks += 1;
                    // Partial refresh is cleaner than Fast; clear ghosting with a full flash now and then.
                    let mode = if self.ticks % 10 == 0 { UpdateMode::Full } else { UpdateMode::Partial };
                    rq.add(RenderData::new(self.id, self.rect, mode));
                }
                true
            },
            Event::Gesture(GestureEvent::Tap(pt)) if self.rect.includes(pt) => {
                if self.tap(pt, hub) {
                    self.ticks += 1;
                    let mode = if self.ticks % 10 == 0 { UpdateMode::Full } else { UpdateMode::Partial };
                    rq.add(RenderData::new(self.id, self.rect, mode));
                }
                true
            },
            _ => false,
        }
    }

    fn render(&self, fb: &mut dyn Framebuffer, _rect: Rectangle, fonts: &mut Fonts) {
        let lay = &self.lay;
        let px = lay.px;
        fb.draw_rectangle(&self.rect, WHITE);
        draw_btn(fb, fonts, &Btn { rect: lay.back, label: "Back".to_string(), on: false }, px);
        draw_btn(fb, fonts, &Btn { rect: lay.pause, label: if self.paused { "Resume".to_string() } else { "Pause".to_string() }, on: self.paused }, px);
        draw_btn(fb, fonts, &Btn { rect: lay.new_game, label: "New".to_string(), on: false }, px);

        // Board.
        let b = lay.board;
        let c = lay.cell;
        fb.draw_rectangle(&rect![b.min.x - 4, b.min.y - 4, b.max.x + 4, b.max.y + 4], BLACK);
        fb.draw_rectangle(&b, WHITE);
        for i in 1..W as i32 {
            fb.draw_rectangle(&rect![b.min.x + i * c, b.min.y, b.min.x + i * c + 1, b.max.y], GRAY13);
        }
        for i in 1..H as i32 {
            fb.draw_rectangle(&rect![b.min.x, b.min.y + i * c, b.max.x, b.min.y + i * c + 1], GRAY13);
        }
        for r in 0..H {
            for col in 0..W {
                let v = self.board[r][col];
                if v != 0 {
                    self.draw_cell(fb, b.min.x + col as i32 * c, b.min.y + r as i32 * c, c, v as usize - 1);
                }
            }
        }
        if !self.over {
            // Ghost outline.
            let mut gy = self.piece.y;
            while !self.hit(&self.piece.m, self.piece.x, gy + 1) { gy += 1; }
            for (r, row) in self.piece.m.iter().enumerate() {
                for (col, &v) in row.iter().enumerate() {
                    if v != 0 {
                        let cr = rect![b.min.x + (self.piece.x + col as i32) * c + 3, b.min.y + (gy + r as i32) * c + 3,
                                       b.min.x + (self.piece.x + col as i32 + 1) * c - 3, b.min.y + (gy + r as i32 + 1) * c - 3];
                        fb.draw_rectangle_outline(&cr, &BorderSpec { thickness: 3, color: GRAY09 });
                    }
                }
            }
            for (r, row) in self.piece.m.iter().enumerate() {
                for (col, &v) in row.iter().enumerate() {
                    if v != 0 {
                        self.draw_cell(fb, b.min.x + (self.piece.x + col as i32) * c, b.min.y + (self.piece.y + r as i32) * c, c, self.piece.kind);
                    }
                }
            }
        }

        // Side panel.
        let p = lay.panel;
        let line_h = (px * 2).max(30);
        let mut y = p.min.y;
        draw_text(fb, fonts, "Next", px, BLACK, &rect![p.min.x, y, p.max.x, y + line_h]);
        y += line_h;
        let nm = shape(self.next);
        let ns = (c * 7 / 10).max(10);
        let nw = nm[0].len() as i32 * ns;
        let ox = p.min.x + (p.width() as i32 - nw) / 2;
        for (r, row) in nm.iter().enumerate() {
            for (col, &v) in row.iter().enumerate() {
                if v != 0 {
                    self.draw_cell(fb, ox + col as i32 * ns, y + r as i32 * ns, ns, self.next);
                }
            }
        }
        y += ns * 3 + line_h / 2;
        for (label, value) in [("Score", self.score), ("Lines", self.lines), ("Level", self.level + 1), ("Best", self.best.max(self.score))] {
            draw_text(fb, fonts, label, px * 4 / 5, BLACK, &rect![p.min.x, y, p.max.x, y + line_h]);
            y += line_h;
            draw_text(fb, fonts, &value.to_string(), px * 6 / 5, BLACK, &rect![p.min.x, y, p.max.x, y + line_h * 3 / 2]);
            y += line_h * 3 / 2 + line_h / 3;
        }
        let _ = LINE;
        if self.over || self.paused {
            let mid = (b.min.y + b.max.y) / 2;
            let br = rect![b.min.x + c, mid - c * 2, b.max.x - c, mid + c * 2];
            fb.draw_rectangle(&br, BLACK);
            fb.draw_rectangle(&rect![br.min.x + 3, br.min.y + 3, br.max.x - 3, br.max.y - 3], WHITE);
            draw_text(fb, fonts, if self.over { "Game over" } else { "Paused" }, px * 3 / 2, BLACK, &br);
        }

        // Controls.
        for (r, label) in [(lay.left, "Left"), (lay.rot, "Rotate"), (lay.right, "Right"), (lay.down, "Down"), (lay.drop, "Drop")] {
            draw_btn(fb, fonts, &Btn { rect: r, label: label.to_string(), on: false }, px * 6 / 5);
        }
    }

    fn resize(&mut self, rect: Rectangle, _hub: &Hub, rq: &mut RenderQueue, _context: &mut Context) {
        self.rect = rect;
        self.lay = layout(rect);
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
