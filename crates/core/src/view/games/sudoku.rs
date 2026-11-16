use std::sync::OnceLock;
use crate::color::{BLACK, WHITE, GRAY09, GRAY11, GRAY13, GRAY14};
use crate::context::Context;
use crate::font::Fonts;
use crate::framebuffer::{Framebuffer, UpdateMode};
use crate::geom::Rectangle;
use crate::gesture::GestureEvent;
use crate::view::{View, Event, Hub, Bus, RenderQueue, RenderData, Id, ID_FEEDER};
use super::{Btn, Rng, split_row, draw_text, draw_btn, LINE};

static DATA: &str = include_str!("../../../data/sudoku.txt");
const LEVELS: [&str; 4] = ["Easy", "Medium", "Hard", "Expert"];

fn puzzles() -> &'static [Vec<&'static str>; 4] {
    static CELL: OnceLock<[Vec<&'static str>; 4]> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut out: [Vec<&'static str>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for line in DATA.lines() {
            let mut it = line.split(' ');
            if let (Some(l), Some(p)) = (it.next(), it.next()) {
                if let Ok(l) = l.parse::<usize>() {
                    if l < 4 && p.len() == 81 {
                        out[l].push(p);
                    }
                }
            }
        }
        out
    })
}

fn candidates(g: &[u8; 81], i: usize) -> u16 {
    let (r, c) = (i / 9, i % 9);
    let mut m: u16 = 0x1ff;
    for k in 0..9 {
        let a = g[r * 9 + k];
        if a > 0 { m &= !(1 << (a - 1)); }
        let b = g[k * 9 + c];
        if b > 0 { m &= !(1 << (b - 1)); }
    }
    let (br, bc) = (r / 3 * 3, c / 3 * 3);
    for dr in 0..3 {
        for dc in 0..3 {
            let v = g[(br + dr) * 9 + bc + dc];
            if v > 0 { m &= !(1 << (v - 1)); }
        }
    }
    m
}

fn solve(g: &mut [u8; 81]) -> bool {
    let mut best = 81;
    let mut best_m = 0u16;
    let mut best_c = 10;
    for i in 0..81 {
        if g[i] == 0 {
            let m = candidates(g, i);
            let c = m.count_ones();
            if c == 0 { return false; }
            if c < best_c {
                best_c = c;
                best = i;
                best_m = m;
            }
        }
    }
    if best == 81 { return true; }
    for d in 1..=9u8 {
        if best_m >> (d - 1) & 1 == 1 {
            g[best] = d;
            if solve(g) { return true; }
        }
    }
    g[best] = 0;
    false
}

fn peers(i: usize) -> Vec<usize> {
    let (r, c) = (i / 9, i % 9);
    (0..81).filter(|&k| {
        let (rr, cc) = (k / 9, k % 9);
        k != i && (rr == r || cc == c || (rr / 3 == r / 3 && cc / 3 == c / 3))
    }).collect()
}

#[derive(Clone)]
struct Layout {
    back: Rectangle,
    levels: Vec<Rectangle>,
    prev: Rectangle,
    label: Rectangle,
    next: Rectangle,
    random: Rectangle,
    grid: Rectangle,
    cell: i32,
    pad: Vec<Rectangle>,
    tools: Vec<Rectangle>,
    status: Rectangle,
    px: i32,
}

fn layout(rect: Rectangle) -> Layout {
    let w = rect.width() as i32;
    let h = rect.height() as i32;
    let m = (w / 28).max(12);
    let gap = (m / 2).max(6);
    let btn = (w / 11).max(48);
    let mut y = rect.min.y + m;
    let (x0, x1) = (rect.min.x + m, rect.max.x - m);
    // Row 1: back + 4 levels.
    let r1 = split_row(x0, x1, y, y + btn, 5, gap);
    y += btn + gap;
    // Row 2: prev, label, next, random.
    let r2_y = y;
    y += btn + gap;
    // Fixed heights below the grid: pad, tools, status.
    let below = btn + gap + btn + gap + btn + m;
    let avail_h = (rect.max.y - y) - below;
    let cell = ((x1 - x0) / 9).min(avail_h / 9).max(20);
    let gw = cell * 9;
    let gx = rect.min.x + (w - gw) / 2;
    let grid = rect![gx, y, gx + gw, y + gw];
    y += gw + gap;
    let pad = split_row(x0, x1, y, y + btn, 9, gap / 2);
    y += btn + gap;
    let tools = split_row(x0, x1, y, y + btn, 6, gap / 2);
    y += btn + gap;
    let status = rect![x0, y, x1, (y + btn).min(rect.max.y - 2)];
    let random = rect![x1 - btn * 2, r2_y, x1, r2_y + btn];
    let next = rect![random.min.x - gap - btn, r2_y, random.min.x - gap, r2_y + btn];
    let prev = rect![x0, r2_y, x0 + btn, r2_y + btn];
    let label = rect![prev.max.x + gap, r2_y, next.min.x - gap, r2_y + btn];
    let _ = h;
    Layout {
        back: r1[0],
        levels: r1[1..].to_vec(),
        prev,
        label,
        next,
        random,
        grid,
        cell,
        pad,
        tools,
        status,
        px: (btn * 2 / 5).max(14),
    }
}

pub struct Sudoku {
    id: Id,
    rect: Rectangle,
    children: Vec<Box<dyn View>>,
    lay: Layout,
    level: usize,
    idx: usize,
    puz: [u8; 81],
    sol: [u8; 81],
    board: [u8; 81],
    notes: [u16; 81],
    sel: Option<usize>,
    pencil: bool,
    hist: Vec<([u8; 81], [u16; 81])>,
    check: bool,
    msg: String,
    rng: Rng,
}

impl Sudoku {
    pub fn new(rect: Rectangle, rq: &mut RenderQueue, _context: &mut Context) -> Sudoku {
        let id = ID_FEEDER.next();
        let mut s = Sudoku {
            id,
            rect,
            children: Vec::new(),
            lay: layout(rect),
            level: 0,
            idx: 0,
            puz: [0; 81],
            sol: [0; 81],
            board: [0; 81],
            notes: [0; 81],
            sel: None,
            pencil: false,
            hist: Vec::new(),
            check: false,
            msg: String::new(),
            rng: Rng::new(),
        };
        s.load(0, 0);
        rq.add(RenderData::new(id, rect, UpdateMode::Full));
        s
    }

    fn total(&self) -> usize {
        puzzles()[self.level].len()
    }

    fn load(&mut self, level: usize, idx: usize) {
        let list = &puzzles()[level];
        if list.is_empty() {
            return;
        }
        self.level = level;
        self.idx = idx % list.len();
        let p = list[self.idx].as_bytes();
        for i in 0..81 {
            self.puz[i] = p[i] - b'0';
        }
        self.board = self.puz;
        let mut sol = self.puz;
        solve(&mut sol);
        self.sol = sol;
        self.notes = [0; 81];
        self.sel = None;
        self.hist.clear();
        self.check = false;
        self.msg.clear();
    }

    fn push_hist(&mut self) {
        self.hist.push((self.board, self.notes));
        if self.hist.len() > 300 {
            self.hist.remove(0);
        }
    }

    fn place(&mut self, d: u8) {
        let i = match self.sel { Some(i) => i, None => return };
        if self.puz[i] != 0 { return; }
        self.check = false;
        if d == 0 {
            self.push_hist();
            self.board[i] = 0;
            self.notes[i] = 0;
        } else if self.pencil {
            if self.board[i] != 0 { return; }
            self.push_hist();
            self.notes[i] ^= 1 << (d - 1);
        } else {
            self.push_hist();
            self.board[i] = d;
            self.notes[i] = 0;
            for p in peers(i) {
                self.notes[p] &= !(1 << (d - 1));
            }
        }
        self.msg = if self.board == self.sol { "Solved!".to_string() } else { String::new() };
    }

    fn cell_at(&self, pt: crate::geom::Point) -> Option<usize> {
        if !self.lay.grid.includes(pt) { return None; }
        let c = ((pt.x - self.lay.grid.min.x) / self.lay.cell) as usize;
        let r = ((pt.y - self.lay.grid.min.y) / self.lay.cell) as usize;
        if r < 9 && c < 9 { Some(r * 9 + c) } else { None }
    }

    fn tap(&mut self, pt: crate::geom::Point, hub: &Hub) -> bool {
        let lay = self.lay.clone();
        if lay.back.includes(pt) {
            hub.send(Event::Back).ok();
            return true;
        }
        for (l, r) in lay.levels.iter().enumerate() {
            if r.includes(pt) {
                self.load(l, 0);
                return true;
            }
        }
        if lay.prev.includes(pt) {
            let n = self.total();
            let i = (self.idx + n - 1) % n.max(1);
            self.load(self.level, i);
            return true;
        }
        if lay.next.includes(pt) {
            let i = self.idx + 1;
            self.load(self.level, i);
            return true;
        }
        if lay.random.includes(pt) {
            let i = self.rng.below(self.total());
            self.load(self.level, i);
            return true;
        }
        if let Some(c) = self.cell_at(pt) {
            self.sel = Some(c);
            self.check = false;
            return true;
        }
        for (k, r) in lay.pad.iter().enumerate() {
            if r.includes(pt) {
                self.place(k as u8 + 1);
                return true;
            }
        }
        for (k, r) in lay.tools.iter().enumerate() {
            if r.includes(pt) {
                match k {
                    0 => self.pencil = !self.pencil,
                    1 => self.place(0),
                    2 => {
                        if let Some((b, n)) = self.hist.pop() {
                            self.board = b;
                            self.notes = n;
                            self.check = false;
                            self.msg.clear();
                        }
                    },
                    3 => {
                        if let Some(i) = self.sel {
                            if self.puz[i] == 0 {
                                self.push_hist();
                                self.board[i] = self.sol[i];
                                self.notes[i] = 0;
                                self.msg = if self.board == self.sol { "Solved!".to_string() } else { String::new() };
                            }
                        }
                    },
                    4 => {
                        self.check = true;
                        let bad = (0..81).filter(|&i| self.puz[i] == 0 && self.board[i] != 0 && self.board[i] != self.sol[i]).count();
                        self.msg = if bad == 0 { "No mistakes so far".to_string() } else { format!("{} wrong", bad) };
                    },
                    _ => {
                        self.board = self.puz;
                        self.notes = [0; 81];
                        self.hist.clear();
                        self.check = false;
                        self.msg.clear();
                    },
                }
                return true;
            }
        }
        false
    }
}

impl View for Sudoku {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, _bus: &mut Bus, rq: &mut RenderQueue, _context: &mut Context) -> bool {
        match *evt {
            Event::Gesture(GestureEvent::Tap(pt)) if self.rect.includes(pt) => {
                if self.tap(pt, hub) {
                    rq.add(RenderData::new(self.id, self.rect, UpdateMode::Gui));
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
        for (l, r) in lay.levels.iter().enumerate() {
            draw_btn(fb, fonts, &Btn { rect: *r, label: LEVELS[l].to_string(), on: l == self.level }, px);
        }
        draw_btn(fb, fonts, &Btn { rect: lay.prev, label: "<".to_string(), on: false }, px);
        draw_text(fb, fonts, &format!("#{} of {}", self.idx + 1, self.total()), px, BLACK, &lay.label);
        draw_btn(fb, fonts, &Btn { rect: lay.next, label: ">".to_string(), on: false }, px);
        draw_btn(fb, fonts, &Btn { rect: lay.random, label: "Random".to_string(), on: false }, px);

        // Grid.
        let g = lay.grid;
        let cell = lay.cell;
        fb.draw_rectangle(&g, LINE);
        let sel_v = self.sel.map(|s| self.board[s]).unwrap_or(0);
        let sel_peers: Vec<usize> = self.sel.map(peers).unwrap_or_default();
        for i in 0..81 {
            let (r, c) = ((i / 9) as i32, (i % 9) as i32);
            let cr = rect![g.min.x + c * cell + 1, g.min.y + r * cell + 1,
                           g.min.x + (c + 1) * cell - 1, g.min.y + (r + 1) * cell - 1];
            let v = self.board[i];
            let given = self.puz[i] != 0;
            let wrong = self.check && !given && v != 0 && v != self.sol[i];
            let (bg, fg) = if wrong {
                (BLACK, WHITE)
            } else if Some(i) == self.sel {
                (GRAY09, BLACK)
            } else if v != 0 && v == sel_v {
                (GRAY11, BLACK)
            } else if sel_peers.contains(&i) {
                (GRAY14, BLACK)
            } else if given {
                (GRAY13, BLACK)
            } else {
                (WHITE, BLACK)
            };
            fb.draw_rectangle(&cr, bg);
            if v != 0 {
                draw_text(fb, fonts, &v.to_string(), cell * 6 / 10, fg, &cr);
            } else if self.notes[i] != 0 {
                let sub = cell / 3;
                for d in 0..9i32 {
                    if self.notes[i] >> d & 1 == 1 {
                        let (dr, dc) = (d / 3, d % 3);
                        let nr = rect![cr.min.x + dc * sub, cr.min.y + dr * sub,
                                       cr.min.x + (dc + 1) * sub, cr.min.y + (dr + 1) * sub];
                        draw_text(fb, fonts, &(d + 1).to_string(), (sub * 6 / 10).max(10), fg, &nr);
                    }
                }
            }
        }
        // Thick box lines.
        let t = (cell / 22).max(3);
        for k in 0..=3 {
            let x = g.min.x + k * cell * 3;
            fb.draw_rectangle(&rect![x - t / 2, g.min.y - t / 2, x + t / 2 + 1, g.max.y + t / 2 + 1], BLACK);
            let y = g.min.y + k * cell * 3;
            fb.draw_rectangle(&rect![g.min.x - t / 2, y - t / 2, g.max.x + t / 2 + 1, y + t / 2 + 1], BLACK);
        }

        // Number pad (greyed out when all nine are placed).
        let mut counts = [0u8; 10];
        for v in self.board.iter() { counts[*v as usize] += 1; }
        for (k, r) in lay.pad.iter().enumerate() {
            let d = k + 1;
            let label = d.to_string();
            draw_btn(fb, fonts, &Btn { rect: *r, label, on: false }, px);
            if counts[d] >= 9 {
                fb.draw_blended_rectangle(r, WHITE, 0.7);
            }
        }
        let labels = [if self.pencil { "Pencil ON" } else { "Pencil" }, "Erase", "Undo", "Hint", "Check", "Restart"];
        for (k, r) in lay.tools.iter().enumerate() {
            draw_btn(fb, fonts, &Btn { rect: *r, label: labels[k].to_string(), on: k == 0 && self.pencil }, px * 4 / 5);
        }
        draw_text(fb, fonts, &format!("{}   {}", LEVELS[self.level], self.msg), px, BLACK, &lay.status);
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
