//! Pure GUI geometry for the per-mode views, in a 210x210 panel; path strings use SVG syntax.

use crate::modes::Mode;
use crate::sequence::{
    hilbert_point, logistic_r, oscillator_value, oscillator_wave_count, Pattern, CA_ROWS, MAX_STEPS,
    TONNETZ_START,
};
use std::f32::consts::TAU;
use std::fmt::Write;

pub const P: f32 = 210.0;
const C: f32 = P / 2.0;
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub on: bool,
    pub cur: bool,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub nodes: Vec<Node>,
    pub edges: String,
    pub fills: String,
    pub lines: String,
    pub solid: String,
    /// Acid piano-roll tone classes: slid notes and octave-jumped notes have their own colour.
    pub slid: String,
    pub jumped: String,
    pub hand: String,
}

impl View {
    /// Every path slot, for the empty-path sentinel and the tests that sweep them.
    pub fn slots(&mut self) -> [&mut String; 7] {
        [&mut self.edges, &mut self.fills, &mut self.lines, &mut self.solid, &mut self.slid, &mut self.jumped, &mut self.hand]
    }
}

fn polar(r: f32, turns: f32) -> (f32, f32) {
    let a = TAU * turns;
    (C + r * a.sin(), C - r * a.cos())
}
fn mv(s: &mut String, x: f32, y: f32) {
    let _ = write!(s, "M{:.1} {:.1} ", x, y);
}
fn ln(s: &mut String, x: f32, y: f32) {
    let _ = write!(s, "L{:.1} {:.1} ", x, y);
}
fn circle(s: &mut String, cx: f32, cy: f32, r: f32) {
    let _ = write!(s, "M{:.1} {:.1} A{r:.1} {r:.1} 0 1 1 {:.1} {:.1} A{r:.1} {r:.1} 0 1 1 {:.1} {:.1} Z ", cx + r, cy, cx - r, cy, cx + r, cy);
}

/// Annular sector as a polygon (subdivided so wide cells still look round).
fn sector(s: &mut String, r0: f32, r1: f32, t0: f32, t1: f32) {
    let seg = (((t1 - t0) * 360.0 / 15.0).ceil() as usize).max(1);
    for k in 0..=seg {
        let (x, y) = polar(r1, t0 + (t1 - t0) * k as f32 / seg as f32);
        if k == 0 { mv(s, x, y) } else { ln(s, x, y) }
    }
    for k in (0..=seg).rev() {
        let (x, y) = polar(r0, t0 + (t1 - t0) * k as f32 / seg as f32);
        ln(s, x, y);
    }
    s.push_str("Z ");
}

fn edge_path(s: &mut String, a: (f32, f32), b: (f32, f32), nr: f32) {
    if (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01 {
        let (dx, dy) = (a.0 - C, a.1 - C);
        let l = (dx * dx + dy * dy).sqrt().max(1.0);
        let (dx, dy) = (dx / l, dy / l);
        let (px, py) = (-dy, dx);
        let (sx, sy) = (a.0 + dx * nr, a.1 + dy * nr);
        let _ = write!(
            s,
            "M{:.1} {:.1} C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1} ",
            sx, sy,
            sx + dx * 30.0 + px * 22.0, sy + dy * 30.0 + py * 22.0,
            sx + dx * 30.0 - px * 22.0, sy + dy * 30.0 - py * 22.0,
            sx + px * 3.0, sy + py * 3.0
        );
        return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / l, dy / l);
    let (px, py) = (-uy, ux);
    let st = (a.0 + ux * nr, a.1 + uy * nr);
    let en = (b.0 - ux * nr, b.1 - uy * nr);
    let ctl = ((a.0 + b.0) / 2.0 + px * l * 0.18, (a.1 + b.1) / 2.0 + py * l * 0.18);
    mv(s, st.0, st.1);
    let _ = write!(s, "Q{:.1} {:.1} {:.1} {:.1} ", ctl.0, ctl.1, en.0, en.1);
    let (tx, ty) = (en.0 - ctl.0, en.1 - ctl.1);
    let tl = (tx * tx + ty * ty).sqrt().max(0.001);
    let (tx, ty) = (tx / tl, ty / tl);
    mv(s, en.0, en.1);
    ln(s, en.0 - tx * 9.0 - ty * 4.5, en.1 - ty * 9.0 + tx * 4.5);
    ln(s, en.0 - tx * 9.0 + ty * 4.5, en.1 - ty * 9.0 - tx * 4.5);
    s.push_str("Z ");
}

fn lattice(a: i32, b: i32) -> (f32, f32) {
    (10.7 + (a as f32 + 0.5 * b as f32) * 29.0, 167.5 - b as f32 * 25.1)
}
fn tri_pts(a: i32, b: i32, up: bool) -> [(f32, f32); 3] {
    if up {
        [lattice(a, b), lattice(a + 1, b), lattice(a, b + 1)]
    } else {
        [lattice(a + 1, b), lattice(a, b + 1), lattice(a + 1, b + 1)]
    }
}
fn poly(s: &mut String, pts: &[(f32, f32)]) {
    for (i, p) in pts.iter().enumerate() {
        if i == 0 { mv(s, p.0, p.1) } else { ln(s, p.0, p.1) }
    }
    s.push_str("Z ");
}
fn rect(s: &mut String, x0: f32, y0: f32, x1: f32, y1: f32) {
    mv(s, x0, y0);
    ln(s, x1, y0);
    ln(s, x1, y1);
    ln(s, x0, y1);
    s.push_str("Z ");
}

fn line(s: &mut String, a: (f32, f32), b: (f32, f32)) {
    mv(s, a.0, a.1);
    ln(s, b.0, b.1);
}

/// Thumbnail paths for the oscillator picker, one per `OscWave` in order. Each path samples the
/// generator's own `oscillator_value`, so a button cannot advertise a shape the mode does not play.
pub fn wave_icon_paths() -> Vec<String> {
    (0..oscillator_wave_count)
        .map(|wave| {
            let mut s = String::new();
            for k in 0..=64 {
                let phase = k as f32 / 64.0;
                let x = 2.0 + phase * 96.0;
                let y = 4.0 + (1.0 - oscillator_value(wave, phase)) * 32.0;
                if k == 0 { mv(&mut s, x, y) } else { ln(&mut s, x, y) }
            }
            s
        })
        .collect()
}

fn y_of(u: f32) -> f32 {
    P - 20.0 - u.clamp(0.0, 1.0) * (P - 40.0)
}
fn x_of(i: usize, n: usize) -> f32 {
    10.0 + (P - 20.0) * i as f32 / n.max(1) as f32
}

/// Generator step that produced display slot `i`: the inverse of the rotation and mirror applied
/// by `sequence::generate`. Views that redraw generator geometry must index through this or they
/// drift from the notes after rotation, mirroring or a new variation.
fn src_index(pat: &Pattern, i: usize, n: usize) -> usize {
    let j = if pat.mirrored { n - 1 - i } else { i };
    (j + n - pat.rotation % n) % n
}

/// Draw the pitch contour through sounding steps.
fn contour(v: &mut View, pat: &Pattern, n: usize) {
    let mut started = false;
    for i in 0..n {
        if !pat.hit[i] { continue; }
        let p = (x_of(i, n), y_of(pat.u[i]));
        if !started { mv(&mut v.lines, p.0, p.1); started = true; } else { ln(&mut v.lines, p.0, p.1) }
    }
}

fn strip_dots(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    for i in 0..n {
        if pat.hit[i] {
            v.nodes.push(Node { x: x_of(i, n), y: y_of(pat.u[i]), r: 4.2, on: true, cur: cur == i as i32, label: String::new() });
        }
    }
}

fn playhead_column(v: &mut View, n: usize, cur: i32) {
    if cur < 0 { return; }
    let x = x_of(cur as usize, n);
    let w = (P - 20.0) / n as f32;
    poly(&mut v.hand, &[(x, 6.0), (x + w, 6.0), (x + w, P - 6.0), (x, P - 6.0)]);
}

/// Acid piano roll: a column per step and a row per pitch. Tone classes get their own path so one
/// note's colour never repaints the rest: octave jump > slide > accent > plain.
fn acid_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let (x0, x1, y0, y1) = (6.0f32, P - 6.0, 12.0f32, P - 12.0);
    let sw = (x1 - x0) / n as f32;
    let pv = |i: usize| pat.oct[i] as f32 + pat.u[i];
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for i in (0..n).filter(|&i| pat.hit[i]) {
        lo = lo.min(pv(i));
        hi = hi.max(pv(i));
    }
    if lo > hi {
        (lo, hi) = (0.0, 1.0);
    }
    let lo = lo.floor();
    let hi = (hi.floor() + 1.0).max(lo + 2.0);
    let y_of = |p: f32| y1 - (p - lo) / (hi - lo) * (y1 - y0);
    let x_of = |i: usize| x0 + i as f32 * sw;

    let mut oc = lo;
    while oc <= hi {
        line(&mut v.edges, (x0, y_of(oc)), (x1, y_of(oc)));
        oc += 1.0;
    }
    for i in (0..n).step_by(4) {
        line(&mut v.edges, (x_of(i), 4.0), (x_of(i), P - 4.0));
    }
    for i in (0..n).filter(|&i| pat.hit[i]) {
        let (xa, xb, y) = (x_of(i) + 0.8, x_of(i) + sw - 0.8, y_of(pv(i)));
        let dst = if pat.oct[i] != 0 {
            &mut v.jumped
        } else if pat.slide[i] {
            &mut v.slid
        } else if pat.acc[i] >= 0.9 {
            &mut v.solid
        } else {
            &mut v.fills
        };
        poly(dst, &[(xa, y - 4.0), (xb, y - 4.0), (xb, y + 4.0), (xa, y + 4.0)]);
        if pat.slide[i] {
            mv(&mut v.lines, xb, y);
            match crate::next_hit_after(&pat.hit, i, n) {
                Some(j) if j > i => ln(&mut v.lines, x_of(j) + 0.8, y_of(pv(j))),
                _ => ln(&mut v.lines, x1, y),
            }
        }
    }
    if cur >= 0 {
        let i = cur as usize;
        let (xa, xb) = (x_of(i), x_of(i) + sw);
        poly(&mut v.hand, &[(xa, 4.0), (xb, 4.0), (xb, P - 4.0), (xa, P - 4.0)]);
    }
}

/// Directed graph over rest (0) plus seven scale degrees. Edges show the walk actually taken.
fn markov_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let pos = |s: usize| polar(78.0, s as f32 / 8.0);
    for s in 1..8 {
        edge_path(&mut v.edges, pos(0), pos(s), 9.0);
    }
    for i in 1..n {
        if pat.state[i] != 0 && pat.state[i - 1] != 0 {
            let a = pos((pat.state[i - 1] - 1) as usize);
            let b = pos((pat.state[i] - 1) as usize);
            line(&mut v.edges, a, b);
        }
    }
    let mut dest = usize::MAX;
    if cur >= 0 {
        let s = pat.state[cur as usize] as usize;
        if s != 0 && cur > 0 && pat.state[cur as usize - 1] != 0 {
            let a = pos((pat.state[cur as usize - 1] - 1) as usize);
            let b = pos((s - 1) as usize);
            edge_path(&mut v.hand, a, b, 9.0);
        }
        if s != 0 { dest = s - 1; }
    }
    for s in 0..8 {
        let (x, y) = pos(s);
        v.nodes.push(Node {
            x, y, r: 11.0, on: s != 0, cur: s == dest,
            label: if s == 0 { "·".into() } else { s.to_string() },
        });
    }
}

/// Oscillator panel: the exact waveform the generator sampled, plus the chosen onsets.
fn oscillator_view(v: &mut View, pat: &Pattern, n: usize, cur: i32, wave: usize) {
    let wave = wave % oscillator_wave_count;
    // The generator's exact curve: display slot `c` maps to generator step `g`. Same y scale as the dots.
    let (cycles, phase0) = pat.osc;
    let mut prev: Option<f32> = None;
    let dense = 240usize;
    for k in 0..=dense {
        let c = k as f32 / dense as f32 * n as f32;
        let g = {
            let j = if pat.mirrored { n as f32 - 1.0 - c } else { c };
            (j - pat.rotation as f32).rem_euclid(n as f32)
        };
        let phase = g / n as f32 * cycles + phase0;
        let y = y_of(crate::sequence::oscillator_value(wave, phase));
        let x = x_of(0, n) + (P - 20.0) * c / n as f32;
        // start a new sub-path at the loop seam instead of drawing a false vertical connector
        let seam = match prev {
            Some(pg) => (g - pg).abs() > n as f32 / 2.0,
            None => true,
        };
        if seam { mv(&mut v.edges, x, y) } else { ln(&mut v.edges, x, y) }
        prev = Some(g);
    }
    strip_dots(v, pat, n, cur);
    contour(v, pat, n);
    playhead_column(v, n, cur);
}

/// L-System panel: the actual A/B word as a strip, with the contour above it.
fn lsystem_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let cols = 16usize;
    let rows = n.div_ceil(cols).max(1);
    let cell = (P - 12.0) / cols as f32;
    let rh = (58.0 / rows as f32).min(cell);
    for i in 0..n {
        let (cx, cy) = (6.0 + (i % cols) as f32 * cell, P - 6.0 - (i / cols) as f32 * rh);
        let a = pat.lsym[i] == 0;
        let dst = if pat.hit[i] { &mut v.solid } else { &mut v.fills };
        poly(dst, &[(cx + 0.6, cy - rh + 0.6), (cx + cell - 0.6, cy - rh + 0.6), (cx + cell - 0.6, cy - 0.6), (cx + 0.6, cy - 0.6)]);
        if a {
            line(&mut v.edges, (cx + cell * 0.25, cy - rh * 0.5), (cx + cell * 0.75, cy - rh * 0.5));
        } else {
            line(&mut v.edges, (cx + cell * 0.5, cy - rh * 0.75), (cx + cell * 0.5, cy - rh * 0.25));
        }
    }
    if cur >= 0 {
        let i = cur as usize;
        let (cx, cy) = (6.0 + (i % cols) as f32 * cell, P - 6.0 - (i / cols) as f32 * rh);
        poly(&mut v.hand, &[(cx, cy - rh), (cx + cell, cy - rh), (cx + cell, cy), (cx, cy)]);
    }
    // Contour lives in the upper half so the word strip stays readable.
    let mut started = false;
    for i in 0..n {
        if !pat.hit[i] { continue; }
        let p = (x_of(i, n), 12.0 + (1.0 - pat.u[i]) * 100.0);
        if !started { mv(&mut v.lines, p.0, p.1); started = true; } else { ln(&mut v.lines, p.0, p.1) }
    }
}

/// Sieve panel: the three residue-class lattice rows and the resulting mask.
fn sieve_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let cls = pat.sieve_cls;
    let rh = 46.0f32;
    for (k, &(m, r)) in cls.iter().enumerate() {
        let y = 20.0 + k as f32 * rh;
        line(&mut v.edges, (8.0, y), (P - 8.0, y));
        for i in 0..n {
            if m == 0 { continue; }
            let x = x_of(i, n);
            if src_index(pat, i, n) % m == r {
                let dst = if pat.hit[i] { &mut v.solid } else { &mut v.fills };
                poly(dst, &[(x - 2.0, y - 2.0), (x + 2.0, y - 2.0), (x + 2.0, y + 2.0), (x - 2.0, y + 2.0)]);
            }
        }
    }
    for i in 0..n {
        if pat.hit[i] {
            v.nodes.push(Node { x: x_of(i, n), y: P - 12.0, r: 3.0, on: true, cur: cur == i as i32, label: String::new() });
        }
    }
    playhead_column(v, n, cur);
}

/// Fibonacci / Sturmian panel: the staircase of floor((i+1)α) against the line of slope α, with
/// onsets marked on the steps.
fn fibonacci_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let total = (0..n).filter(|&i| pat.hit[i]).count();
    let top = (total as f32).max(pat.map_param * n as f32).max(1.0);
    let yc = |count: f32| y_of(count / top);
    let mut count = 0.0f32;
    mv(&mut v.edges, x_of(0, n), yc(0.0));
    for i in 0..n {
        let x = x_of(i, n);
        if pat.hit[i] {
            ln(&mut v.edges, x, yc(count));
            count += 1.0;
            ln(&mut v.edges, x, yc(count));
            v.nodes.push(Node { x, y: yc(count), r: 4.0, on: true, cur: cur == i as i32, label: String::new() });
        }
    }
    ln(&mut v.edges, x_of(n, n), yc(count));

    line(&mut v.lines, (x_of(0, n), yc(0.0)), (x_of(n, n), yc(total as f32)));
    playhead_column(v, n, cur);
}

/// Bernoulli panel: each step is a coin flip; hits sit on the contour, misses on the baseline.
fn bernoulli_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    line(&mut v.edges, (8.0, P - 12.0), (P - 8.0, P - 12.0));
    for i in 0..n {
        let x = x_of(i, n);
        if pat.hit[i] {
            v.nodes.push(Node { x, y: y_of(pat.u[i]), r: 4.5, on: true, cur: cur == i as i32, label: String::new() });
        } else {
            v.nodes.push(Node { x, y: P - 12.0, r: 1.8, on: false, cur: false, label: String::new() });
        }
    }
    contour(v, pat, n);
    playhead_column(v, n, cur);
}

/// Golomb panel: ruler marks with an arc over every pair, its height proportional to the interval.
/// In a Golomb ruler no two arcs have the same width.
fn golomb_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    let y = P * 0.5;
    line(&mut v.edges, (8.0, y), (P - 8.0, y));
    let marks: Vec<usize> = (0..n).filter(|&i| pat.hit[i]).collect();
    for &i in &marks {
        let x = x_of(i, n);
        line(&mut v.edges, (x, y - 10.0), (x, y + 10.0));
        v.nodes.push(Node { x, y, r: 5.0, on: true, cur: cur == i as i32, label: String::new() });
    }

    let all_pairs = marks.len() <= 12;
    for (a, &i) in marks.iter().enumerate() {
        let last = if all_pairs { marks.len() } else { (a + 2).min(marks.len()) };
        for &j in &marks[a + 1..last] {
            let (xa, xb) = (x_of(i, n), x_of(j, n));
            let h = 6.0 + 62.0 * (xb - xa) / (P - 20.0);
            mv(&mut v.lines, xa, y - 10.0);
            let _ = write!(v.lines, "Q{:.1} {:.1} {:.1} {:.1} ", (xa + xb) / 2.0, y - 10.0 - 2.0 * h, xb, y - 10.0);
        }
    }
}

/// Dice panel: one cell per bar; the number rolled is the height, hits are filled.
fn dice_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    for bar in 0..n.div_ceil(4) {
        let x0 = 8.0 + bar as f32 * (P - 16.0) / n.div_ceil(4) as f32;
        let w = (P - 16.0) / n.div_ceil(4) as f32 - 2.0;
        for j in 0..4 {
            let i = bar * 4 + j;
            if i >= n { break; }
            let h = 10.0 + pat.roll[i] as f32 * 6.0;
            let y = P - 12.0 - h;
            let dst = if pat.hit[i] { &mut v.solid } else { &mut v.fills };
            let x = x0 + w * j as f32 / 4.0;
            poly(dst, &[(x, y), (x + w / 4.0 - 1.0, y), (x + w / 4.0 - 1.0, P - 12.0), (x, P - 12.0)]);
            if pat.hit[i] {
                v.nodes.push(Node { x: x + w / 8.0, y, r: 3.0, on: true, cur: cur == i as i32, label: String::new() });
            }
        }
    }
}

/// Logistic panel: the map's parabola and the orbit points the generator actually visited.
fn logistic_view(v: &mut View, pat: &Pattern, n: usize, cur: i32) {
    // r comes from the generator's own pattern snapshot.
    let r = if pat.map_param > 0.0 { pat.map_param } else { logistic_r(0.65) as f32 };
    let (bx, by, w, hgt) = (18.0, P - 63.0, P - 36.0, P - 85.0);
    mv(&mut v.edges, bx, by); ln(&mut v.edges, bx + w, by);
    mv(&mut v.edges, bx, by); ln(&mut v.edges, bx, by - hgt);
    // the diagonal x_{n+1} = x_n used by the cobweb
    line(&mut v.edges, (bx, by), (bx + w, by - hgt));
    for s in 0..=48 {
        let x = s as f32 / 48.0;
        let y = (r * x * (1.0 - x)).clamp(0.0, 1.0);
        let px = bx + x * w;
        let py = by - y * hgt;
        if s == 0 { mv(&mut v.edges, px, py); } else { ln(&mut v.edges, px, py); }
    }
    for i in 0..n {
        // (x_n, x_{n+1}) from the stored successor, on the parabola for any rotation or mirror.
        let (x, y) = (pat.orbit[i], pat.orbit_next[i]);
        let (px, py) = (bx + x * w, by - y * hgt);
        let on = pat.hit[i];
        if on {
            // cobweb: up from the diagonal to the curve, then across to the next x
            mv(&mut v.lines, px, by - x * hgt);
            ln(&mut v.lines, px, py);
            ln(&mut v.lines, bx + y * w, py);
        }
        v.nodes.push(Node { x: px, y: py, r: if on { 4.0 } else { 2.0 }, on, cur: cur == i as i32, label: String::new() });
    }

    for i in 0..n {
        let ox = 12.0 + (i as f32 + 0.5) / n as f32 * (P - 24.0);
        let oy = P - 12.0 - pat.orbit[i] * 32.0;
        if i == 0 { mv(&mut v.lines, ox, oy); } else { ln(&mut v.lines, ox, oy); }
    }
}

/// Automatic binary words (Thue-Morse, Rudin-Shapiro): a two-line symbol strip plus contour.
fn binary_word_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    let cols = 16usize;
    let rows = n.div_ceil(cols).max(1);
    let cell = (P - 12.0) / cols as f32;
    let rh = (58.0 / rows as f32).min(cell);
    for i in 0..n {
        let (cx, cy) = (6.0 + (i % cols) as f32 * cell, P - 6.0 - (i / cols) as f32 * rh);
        let bit = pat.lsym[i] & 1;
        let dst = if pat.hit[i] { &mut v.solid } else { &mut v.fills };
        poly(dst, &[(cx + 0.6, cy - rh + 0.6), (cx + cell - 0.6, cy - rh + 0.6), (cx + cell - 0.6, cy - 0.6), (cx + 0.6, cy - 0.6)]);
        if bit == 1 {
            line(&mut v.edges, (cx + cell * 0.2, cy - rh * 0.5), (cx + cell * 0.8, cy - rh * 0.5));
        } else {
            line(&mut v.edges, (cx + cell * 0.5, cy - rh * 0.75), (cx + cell * 0.5, cy - rh * 0.25));
        }
    }
    if cur >= 0 {
        let i = cur as usize;
        let (cx, cy) = (6.0 + (i % cols) as f32 * cell, P - 6.0 - (i / cols) as f32 * rh);
        poly(&mut v.hand, &[(cx, cy - rh), (cx + cell, cy - rh), (cx + cell, cy), (cx, cy)]);
    }
    let mut started = false;
    for i in 0..n {
        let p = (x_of(i, n), 12.0 + (1.0 - pat.u[i]) * 100.0);
        if !started { mv(&mut v.lines, p.0, p.1); started = true; } else { ln(&mut v.lines, p.0, p.1) }
    }
}

/// Random-walk panels: the path through every step, with the selected onsets emphasized.
fn walk_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    let mut started = false;
    for i in 0..n {
        let p = (x_of(i, n), y_of(pat.orbit[i]));
        if !started { mv(&mut v.lines, p.0, p.1); started = true; } else { ln(&mut v.lines, p.0, p.1) }
        if pat.hit[i] {
            v.nodes.push(Node { x: p.0, y: p.1, r: 4.0, on: true, cur: cur == i as i32, label: String::new() });
        }
    }
    playhead_column(v, n, cur);
}

/// Tent map: the map curve for the current μ and the orbit points the generator visited.
fn tent_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    let mu = if pat.map_param > 0.0 { pat.map_param } else { pat.nv as f32 / 1000.0 }.clamp(1.0, 2.0);
    let mut started = false;
    for k in 0..=80 {
        let x = k as f32 / 80.0;
        let y = mu * x.min(1.0 - x);
        let p = (12.0 + x * (P - 24.0), P - 12.0 - y * (P - 24.0));
        if !started { mv(&mut v.edges, p.0, p.1); started = true; } else { ln(&mut v.edges, p.0, p.1) }
    }
    for i in 0..n {
        let x = pat.orbit[i];
        let y = mu * x.min(1.0 - x);
        let p = (12.0 + x * (P - 24.0), P - 12.0 - y * (P - 24.0));
        v.nodes.push(Node { x: p.0, y: p.1, r: if pat.hit[i] { 4.0 } else { 1.8 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
    }
    playhead_column(v, n, cur);
}

/// Hénon attractor: the normalized (x, y) orbit in a square, not a time strip.
fn henon_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    poly(&mut v.edges, &[(10.0, 10.0), (P - 10.0, 10.0), (P - 10.0, P - 10.0), (10.0, P - 10.0)]);
    for i in 0..n {
        let p = (10.0 + pat.orbit[i] * (P - 20.0), P - 10.0 - pat.theta[i] * (P - 20.0));
        v.nodes.push(Node { x: p.0, y: p.1, r: if pat.hit[i] { 4.0 } else { 1.8 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
    }
    playhead_column(v, n, cur);
}

/// Absolute-pitch modes (arpeggiator, walking bass): a compact piano roll of `semis`.
fn absolute_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    let (mut lo, mut hi) = (0i32, 12i32);
    for i in 0..n {
        if pat.hit[i] {
            lo = lo.min(pat.semis[i] as i32);
            hi = hi.max(pat.semis[i] as i32 + 1);
        }
    }
    lo = lo.div_euclid(12) * 12;
    hi = (hi.div_euclid(12) + 1) * 12;
    if hi - lo < 12 { hi = lo + 12; }
    let rows = (hi - lo).max(1) as f32;
    let rh = (P - 24.0) / rows;
    let y_of = |p: i32| 12.0 + (hi - 1 - p) as f32 * rh;
    for p in (lo..hi).step_by(12) {
        line(&mut v.edges, (8.0, y_of(p)), (P - 8.0, y_of(p)));
    }
    let sw = (P - 16.0) / n.max(1) as f32;
    for i in 0..n {
        if !pat.hit[i] { continue; }
        let x = 8.0 + i as f32 * sw;
        let y = y_of(pat.semis[i] as i32);
        let dst = if pat.acc[i] >= 0.9 { &mut v.solid } else { &mut v.fills };
        poly(dst, &[(x + 0.8, y - rh * 0.35), (x + sw - 0.8, y - rh * 0.35), (x + sw - 0.8, y + rh * 0.35), (x + 0.8, y + rh * 0.35)]);
    }
    playhead_column(v, n, cur);
}

/// Hilbert curve: a real 2-D space-filling path, with the sampled steps highlighted on it.
fn hilbert_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    let order = (pat.nv.clamp(1, 5)) as u32;
    let size = 1u32 << order;
    let total = size * size;
    let grid = |gx: u32, gy: u32| -> (f32, f32) {
        if size <= 1 {
            (C, C)
        } else {
            (10.0 + gx as f32 * (P - 20.0) / (size - 1) as f32, P - 10.0 - gy as f32 * (P - 20.0) / (size - 1) as f32)
        }
    };
    for k in 0..=size {
        let q = 10.0 + k as f32 * (P - 20.0) / size as f32;
        line(&mut v.edges, (q, 10.0), (q, P - 10.0));
        line(&mut v.edges, (10.0, P - q), (P - 10.0, P - q));
    }
    let mut prev: Option<(f32, f32)> = None;
    for d in 0..total {
        let (gx, gy) = hilbert_point(order, d);
        let p = grid(gx, gy);
        if let Some(q) = prev { line(&mut v.edges, q, p); }
        prev = Some(p);
    }
    for i in 0..n {
        let p = (10.0 + pat.orbit[i] * (P - 20.0), P - 10.0 - pat.theta[i] * (P - 20.0));
        v.nodes.push(Node { x: p.0, y: p.1, r: if pat.hit[i] { 3.8 } else { 1.5 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
    }
    playhead_column(v, n, cur);
}

/// Ulam spiral: a square spiral plot with prime cells and the chosen onsets marked.
fn ulam_view(v: &mut View, pat: &crate::sequence::Pattern, n: usize, cur: i32) {
    poly(&mut v.edges, &[(2.0, 2.0), (P - 2.0, 2.0), (P - 2.0, P - 2.0), (2.0, P - 2.0)]);
    let mut started = false;
    for i in 0..n {
        let p = (2.0 + pat.orbit[i] * (P - 4.0), P - 2.0 - pat.theta[i] * (P - 4.0));
        if !started { mv(&mut v.lines, p.0, p.1); started = true; } else { ln(&mut v.lines, p.0, p.1) }
        if pat.state[i] == 1 || pat.hit[i] {
            v.nodes.push(Node { x: p.0, y: p.1, r: if pat.hit[i] { 4.0 } else { 2.0 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
        }
    }
    playhead_column(v, n, cur);
}

pub fn build(kind: Mode, pat: &crate::sequence::Pattern, steps: usize, cur: i32, root: i32, osc_wave: usize) -> View {
    let n = steps.clamp(1, MAX_STEPS);
    let cur = if cur >= 0 && (cur as usize) < n { cur } else { -1 };
    let mut v = View::default();

    match kind {
        Mode::Euclidean => {
            circle(&mut v.edges, C, C, 82.0);
            let mut hits: Vec<(f32, f32)> = Vec::new();
            for i in 0..n {
                let (x, y) = polar(82.0, i as f32 / n as f32);
                if pat.hit[i] { hits.push((x, y)); }
                v.nodes.push(Node { x, y, r: if pat.hit[i] { 7.0 } else { 3.5 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
            }
            if hits.len() >= 2 {
                for (k, h) in hits.iter().enumerate() {
                    if k == 0 { mv(&mut v.fills, h.0, h.1) } else { ln(&mut v.fills, h.0, h.1) }
                }
                v.fills.push_str("Z ");
            }
            if cur >= 0 {
                let (x, y) = polar(82.0, cur as f32 / n as f32);
                line(&mut v.hand, (C, C), (x, y));
            }
        }
        Mode::Golomb => golomb_view(&mut v, pat, n, cur),
        Mode::Logistic => logistic_view(&mut v, pat, n, cur),
        Mode::Eulerian => {
            let nv = pat.nv.max(1);
            let pos = |i: usize| polar(78.0, i as f32 / nv as f32);
            for k in 0..pat.ne {
                let (a, b) = pat.circuit[k];
                edge_path(&mut v.edges, pos(a as usize), pos(b as usize), 11.0);
            }
            let mut dest = usize::MAX;
            if cur >= 0 {
                let (a, b) = pat.circuit[pat.step_edge[cur as usize] as usize];
                dest = b as usize;
                edge_path(&mut v.hand, pos(a as usize), pos(b as usize), 11.0);
            }
            for i in 0..nv {
                let (x, y) = pos(i);
                v.nodes.push(Node { x, y, r: 11.0, on: i != 0, cur: i == dest, label: if i == 0 { "R".into() } else { i.to_string() } });
            }
        }
        Mode::CircleMap => {
            circle(&mut v.edges, C, C, 96.0);
            circle(&mut v.edges, C, C, 30.0);
            line(&mut v.edges, (C, C), (C, C - 96.0));
            let pts: Vec<(f32, f32)> = (0..n).map(|i| polar(30.0 + 66.0 * (i as f32 + 1.0) / n as f32, pat.theta[i])).collect();
            for (i, p) in pts.iter().enumerate() {
                if i == 0 { mv(&mut v.lines, p.0, p.1) } else { ln(&mut v.lines, p.0, p.1) }
                v.nodes.push(Node { x: p.0, y: p.1, r: if pat.hit[i] { 4.5 } else { 2.0 }, on: pat.hit[i], cur: cur == i as i32, label: String::new() });
            }
            if cur >= 0 {
                line(&mut v.hand, (C, C), pts[cur as usize]);
            }
        }
        Mode::Dice => dice_view(&mut v, pat, n, cur),
        Mode::Tonnetz => {
            for b in 0..=5 {
                for a in 0..=4 {
                    let (x, y) = lattice(a, b);
                    if a < 4 { line(&mut v.edges, (x, y), lattice(a + 1, b)); }
                    if b < 5 { line(&mut v.edges, (x, y), lattice(a, b + 1)); }
                    if a < 4 && b < 5 { line(&mut v.edges, lattice(a + 1, b), lattice(a, b + 1)); }
                }
            }
            let mut seen: Vec<(u8, u8, bool)> = Vec::new();
            let mut visited_nodes: Vec<(i32, i32)> = Vec::new();
            let mut last_c: Option<(f32, f32)> = None;
            for i in 0..n {
                let t = pat.tri[i];
                let pts = tri_pts(t.0 as i32, t.1 as i32, t.2);
                if !seen.contains(&t) {
                    seen.push(t);
                    poly(&mut v.fills, &pts);
                }
                let cen = ((pts[0].0 + pts[1].0 + pts[2].0) / 3.0, (pts[0].1 + pts[1].1 + pts[2].1) / 3.0);
                if last_c != Some(cen) {
                    if last_c.is_none() { mv(&mut v.lines, cen.0, cen.1) } else { ln(&mut v.lines, cen.0, cen.1) }
                    last_c = Some(cen);
                }
                let (a, b) = (t.0 as i32, t.1 as i32);
                let nodes3 = if t.2 { [(a, b), (a + 1, b), (a, b + 1)] } else { [(a + 1, b), (a, b + 1), (a + 1, b + 1)] };
                visited_nodes.extend_from_slice(&nodes3);
            }
            let mut cur_nodes: Vec<(i32, i32)> = Vec::new();
            if cur >= 0 {
                let t = pat.tri[cur as usize];
                poly(&mut v.hand, &tri_pts(t.0 as i32, t.1 as i32, t.2));
                let (a, b) = (t.0 as i32, t.1 as i32);
                cur_nodes = if t.2 { vec![(a, b), (a + 1, b), (a, b + 1)] } else { vec![(a + 1, b), (a, b + 1), (a + 1, b + 1)] };
            }
            let start_pc = (7 * TONNETZ_START.0 + 4 * TONNETZ_START.1).rem_euclid(12);
            for b in 0..=5 {
                for a in 0..=4 {
                    let (x, y) = lattice(a, b);
                    let pc = ((7 * a + 4 * b).rem_euclid(12) - start_pc + root).rem_euclid(12) as usize;
                    v.nodes.push(Node { x, y, r: 8.0, on: visited_nodes.contains(&(a, b)), cur: cur_nodes.contains(&(a, b)), label: NAMES[pc].into() });
                }
            }
        }
        Mode::Cellular => {
            let (rin, rout) = (24.0f32, 100.0f32);
            let rw = (rout - rin) / CA_ROWS as f32;
            for g in 0..=CA_ROWS {
                circle(&mut v.edges, C, C, rin + g as f32 * rw);
            }
            for g in 0..CA_ROWS {
                for j in 0..n {
                    if pat.ca[g][j] {
                        let dst = if g == pat.ca_play { &mut v.solid } else { &mut v.fills };
                        sector(dst, rin + g as f32 * rw + 0.4, rin + (g + 1) as f32 * rw - 0.4, (j as f32 + 0.04) / n as f32, (j as f32 + 0.96) / n as f32);
                    }
                }
            }
            if cur >= 0 {
                let j = cur as f32;
                let g = pat.ca_play as f32;
                sector(&mut v.hand, rin + g * rw, rin + (g + 1.0) * rw, j / n as f32, (j + 1.0) / n as f32);
                line(&mut v.hand, polar(rin, (j + 0.5) / n as f32), polar(rout, (j + 0.5) / n as f32));
            }
        }
        Mode::Markov => markov_view(&mut v, pat, n, cur),
        Mode::Oscillator => oscillator_view(&mut v, pat, n, cur, osc_wave),
        Mode::LSystem => lsystem_view(&mut v, pat, n, cur),
        Mode::Sieve => sieve_view(&mut v, pat, n, cur),
        Mode::Fibonacci => fibonacci_view(&mut v, pat, n, cur),
        Mode::Bernoulli => bernoulli_view(&mut v, pat, n, cur),
        Mode::AcidRandom | Mode::AcidEuclid | Mode::AcidMarkov | Mode::AcidMotif | Mode::AcidShift => {
            acid_view(&mut v, pat, n, cur);
        }
        Mode::ThueMorse | Mode::RudinShapiro => binary_word_view(&mut v, pat, n, cur),
        Mode::Brownian | Mode::LevyFlight => walk_view(&mut v, pat, n, cur),
        Mode::TentMap => tent_view(&mut v, pat, n, cur),
        Mode::HenonMap => henon_view(&mut v, pat, n, cur),
        Mode::Arpeggiator => acid_view(&mut v, pat, n, cur),
        Mode::WalkingBass => absolute_view(&mut v, pat, n, cur),
        Mode::HilbertCurve => hilbert_view(&mut v, pat, n, cur),
        Mode::UlamPrimes => ulam_view(&mut v, pat, n, cur),
        Mode::Life => life_view(&mut v, pat, cur),
    }
    for s in v.slots() {
        if s.is_empty() {
            s.push_str("M0 0 ");
        }
    }
    v
}

/// 16 × 8 Life torus; only the live cells are filled, with a border and a playhead column.
fn life_view(v: &mut View, pat: &Pattern, cur: i32) {
    let (x0, y0) = (25.0f32, 35.0f32);
    let (w, h) = (160.0f32, 112.0f32);
    let cw = w / crate::sequence::life::WIDTH as f32;
    let ch = h / crate::sequence::life::HEIGHT as f32;
    rect(&mut v.edges, x0, y0, x0 + w, y0 + h);
    for row in 0..crate::sequence::life::HEIGHT {
        for col in 0..crate::sequence::life::WIDTH {
            if pat.ca[row][col] {
                let x = x0 + col as f32 * cw;
                let y = y0 + row as f32 * ch;
                rect(&mut v.fills, x + 0.6, y + 0.6, x + cw - 0.6, y + ch - 0.6);
            }
        }
    }
    if cur >= 0 {
        let col = (cur as usize) % crate::sequence::life::WIDTH;
        let x = x0 + col as f32 * cw;
        rect(&mut v.hand, x, y0, x + cw, y0 + h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::{generate, GenParams};

    #[test]
    fn every_mode_builds_with_and_without_playhead() {
        for kind in Mode::all() {
            for steps in [1usize, 7, 16, 32] {
                for pulses in [1usize, 5, 32] {
                    let g = GenParams { kind, steps, pulses, rotate: 3, seed: 0.3, variation: 2, ..GenParams::default() };
                    let pat = generate(&g);
                    for cur in [-1, 0, steps as i32 - 1, 99] {
                        let mut v = build(kind, &pat, steps, cur, 0, 0);
                        for s in v.slots() {
                            assert!(!s.contains("NaN") && !s.contains("inf"), "{kind:?} steps {steps}");
                            assert!(s.starts_with('M'), "{kind:?} empty path");
                        }
                        assert!(v.nodes.iter().all(|n| n.x.is_finite() && n.y.is_finite()), "{kind:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn wave_icons_cover_every_waveform_with_distinct_finite_paths() {
        let paths = wave_icon_paths();
        assert_eq!(paths.len(), oscillator_wave_count);
        for (i, path) in paths.iter().enumerate() {
            assert!(path.starts_with('M'), "wave {i} has no path");
            assert!(!path.contains("NaN") && !path.contains("inf"), "wave {i} is not finite");
        }
        for (i, a) in paths.iter().enumerate() {
            for (j, b) in paths.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "waves {i} and {j} draw the same icon");
            }
        }
    }

    fn count(path: &str) -> usize {
        if path.trim() == "M0 0" { 0 } else { path.matches('M').count() }
    }

    #[test]
    fn acid_accents_are_drawn_solid() {
        for kind in [Mode::AcidRandom, Mode::AcidEuclid, Mode::AcidMarkov, Mode::AcidMotif, Mode::AcidShift] {
            let g = GenParams { kind, steps: 32, pulses: 24, density: 0.8, accent: 1.0, slide: 0.0, octave: 0.0, ..GenParams::default() };
            let pat = generate(&g);
            let v = build(kind, &pat, 32, -1, 0, 0);
            assert!(count(&v.solid) > 0, "{kind:?}: accent 100% drew no solid note");
            let plain = generate(&GenParams { accent: 0.0, ..g });
            let none = build(kind, &plain, 32, -1, 0, 0);
            assert_eq!(count(&none.solid), 0, "{kind:?}: accent 0% still drew solid notes");
            assert!(count(&none.fills) > 0, "{kind:?}: plain notes lost their fill");
            // Unused tone classes must stay empty so they cannot paint over the others.
            assert_eq!(count(&none.slid), 0, "{kind:?}: slide 0% drew slid notes");
            assert_eq!(count(&none.jumped), 0, "{kind:?}: octave 0% drew jumped notes");
        }
    }

    #[test]
    fn acid_tone_classes_are_mutually_exclusive() {
        // One knob saturating at a time: each note lands in exactly one tone class.
        for (slide, octave, slot) in [(1.0f32, 0.0f32, 4usize), (0.0, 1.0, 5), (0.0, 0.0, 3)] {
            let g = GenParams {
                kind: Mode::AcidEuclid, steps: 32, pulses: 24, density: 0.8,
                accent: 1.0, slide, octave, ..GenParams::default()
            };
            let mut v = build(g.kind, &generate(&g), 32, -1, 0, 0);
            let paths: Vec<usize> = v.slots()[..6].iter().map(|s| count(s)).collect();
            // slots order: edges, fills, lines, solid, slid, jumped
            assert!(paths[slot] > 0, "slide {slide} octave {octave}: class {slot} is empty");
            for (k, &n) in paths.iter().enumerate() {
                if k != slot && (k == 3 || k == 4 || k == 5) {
                    assert_eq!(n, 0, "slide {slide} octave {octave}: class {k} also drew notes");
                }
            }
        }
    }

    #[test]
    fn sieve_rows_follow_rotation_and_mirror() {
        // every onset must sit on a residue row, however the line was rotated
        for (rotate, mirror, variation) in [(0usize, false, 0u32), (3, false, 0), (0, true, 0), (5, true, 4), (0, false, 7)] {
            let g = GenParams { kind: Mode::Sieve, steps: 32, pulses: 2, density: 1.0, rotate, mirror, variation, ..GenParams::default() };
            let pat = generate(&g);
            let v = build(Mode::Sieve, &pat, 32, -1, 0, 0);
            let on_row = v.solid.matches('M').count();
            let hits = (0..32).filter(|&i| pat.hit[i]).count();

            assert!(on_row >= hits, "rotate {rotate} mirror {mirror} var {variation}: {on_row} solid cells for {hits} onsets");
        }
    }

    #[test]
    fn logistic_panel_uses_the_generators_r_and_puts_dots_on_the_curve() {
        for density in [0.1f32, 0.65, 1.0] {
            for mirror in [false, true] {
                let g = GenParams { kind: Mode::Logistic, steps: 32, pulses: 12, density, mirror, rotate: 3, ..GenParams::default() };
                let pat = generate(&g);
                let r = pat.map_param;
                assert!((r as f64 - crate::sequence::logistic_r(density)).abs() < 1e-6);
                let v = build(Mode::Logistic, &pat, 32, -1, 0, 0);
                assert!(!v.nodes.is_empty());

                let (bx, by, w, hgt) = (18.0, P - 63.0, P - 36.0, P - 85.0);
                for n in &v.nodes {
                    let x = (n.x - bx) / w;
                    let y = (by - n.y) / hgt;
                    assert!((y - r * x * (1.0 - x)).abs() < 2e-3, "density {density} mirror {mirror}: ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn oscillator_curve_passes_through_the_dots() {
        for (rate, phase, rotate, mirror) in [(1.0f32, 0.0f32, 0usize, false), (2.0, 0.3, 0, false), (3.5, 0.1, 5, false), (2.0, 0.0, 3, true)] {
            let g = GenParams { kind: Mode::Oscillator, steps: 16, pulses: 16, density: 1.0, osc_rate: rate, phase, rotate, mirror, osc_wave: 0, ..GenParams::default() };
            let pat = generate(&g);
            let v = build(Mode::Oscillator, &pat, 16, -1, 0, 0);

            let pts: Vec<(f32, f32)> = v.edges.split('M').flat_map(|seg| seg.split('L')).filter_map(|t| {
                let mut it = t.split_whitespace().map(|x| x.parse::<f32>());
                Some((it.next()?.ok()?, it.next()?.ok()?))
            }).collect();
            assert!(pts.len() > 100);
            for n in &v.nodes {
                let d = pts.iter().map(|p| ((p.0 - n.x).powi(2) + (p.1 - n.y).powi(2)).sqrt()).fold(f32::MAX, f32::min);
                assert!(d < 2.5, "rate {rate} phase {phase} rotate {rotate} mirror {mirror}: dot {d} units off the curve");
            }
        }
    }

    #[test]
    fn golomb_panel_arcs_scale_with_the_interval() {
        let g = GenParams { kind: Mode::Golomb, steps: 64, pulses: 5, density: 1.0, ..GenParams::default() };
        let v = build(Mode::Golomb, &generate(&g), 64, -1, 0, 0);
        assert_eq!(count(&v.lines), 10, "5 marks -> C(5,2) arcs");
    }

    #[test]
    fn acid_view_draws_notes_slides_and_playhead() {
        let g = GenParams { kind: Mode::AcidEuclid, steps: 16, pulses: 12, slide: 1.0, accent: 0.5, ..GenParams::default() };
        let pat = generate(&g);
        let v = build(g.kind, &pat, 16, 3, 0, 0);
        assert!(v.nodes.is_empty());
        let notes_drawn = v.solid.trim() != "M0 0" || v.fills.trim() != "M0 0" || v.slid.trim() != "M0 0";
        assert!(notes_drawn, "accented, plain and slid notes all draw note bars");
        assert_ne!(v.lines.trim(), "M0 0", "slides draw connectors");
        assert_ne!(v.hand.trim(), "M0 0", "playhead column");
        let idle = build(g.kind, &pat, 16, -1, 0, 0);
        assert_eq!(idle.hand.trim(), "M0 0");
    }
}
