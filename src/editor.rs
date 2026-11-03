//! Slint GUI adapter between pure geometry and UI glue. Everything here runs on the editor
//! thread; `process()` never sees Slint and the editor regenerates only when a snapshot changes.
use crate::globals;
use crate::history;
use crate::keys;
use crate::midi_export;
use crate::sequence::{generate, GenParams, Pattern, Rng, MAX_STEPS, MIN_MAX_STEPS};
use crate::view::{self, View};
use crate::modulation::{format_target_value, target_in_mask, LFO_COUNT, LFO_MAX_HZ};
use crate::{
    cell_count_for, scope, LfoPolarity, LfoRateMode, LfoTarget, MelodyParams, Mode, ModTarget,
    OscWave, Rate, Shared, Theme as PluginTheme, Tuning,
};
use nice_plug::prelude::*;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use slint_baseview::{default_gl_config, Size, SlintWindow, WindowHandle, WindowOpenOptions, WindowScalePolicy};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

slint::include_modules!();
const WIDTH: u32 = 860;
const HEIGHT: u32 = 675;
/// Drawing size of the history roll, matching the Slint path viewbox.
const HISTORY_W: f32 = 800.0;
const HISTORY_H: f32 = 200.0;
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/// History length is expressed in 4/4 bars; the host tempo supplies the beat duration.
const BEATS_PER_BAR: f32 = 4.0;

/// Append one technical line to `nelody-seq.log` in the user's home directory. GUI thread only.
fn log_line(text: &str) {
    use std::io::Write;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let Some(home) = home else { return };
    let path = PathBuf::from(home).join("nelody-seq.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", text);
    }
}

/// One row of the flattened, grouped mode list. Header rows carry index -1.
struct ModeEntry {
    group: SharedString,
    index: i32,
    header: bool,
}

/// Sorted, grouped mode list for the dropdown. Header rows carry index -1; option rows carry the
/// stable host index so sorting never changes what the host sees.
fn mode_entries() -> Vec<ModeEntry> {
    let mut entries = Vec::new();
    let mut last_group = "";
    for mode in Mode::display_order() {
        let group = mode.group().label();
        if group != last_group {
            entries.push(ModeEntry { group: SharedString::from(group), index: -1, header: true });
            last_group = group;
        }
        entries.push(ModeEntry { group: SharedString::new(), index: mode.to_index() as i32, header: false });
    }
    entries
}

/// Pair grouped entries into two-column rows: headers stay full-width, modes fill left/right slots.
fn mode_rows() -> Vec<ModeRow> {
    let entries = mode_entries();
    let mut rows = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        if !entries[i].header {
            i += 1;
            continue;
        }
        rows.push(ModeRow {
            header: true,
            group: entries[i].group.clone(),
            left: -1,
            right: -1,
            has_right: false,
        });
        i += 1;
        let start = i;
        while i < entries.len() && !entries[i].header {
            i += 1;
        }
        let items = &entries[start..i];
        let mut j = 0;
        while j < items.len() {
            let has_right = j + 1 < items.len();
            rows.push(ModeRow {
                header: false,
                group: SharedString::new(),
                left: items[j].index,
                right: if has_right { items[j + 1].index } else { -1 },
                has_right,
            });
            j += 2;
        }
    }
    rows
}

pub fn create(params: Arc<MelodyParams>, shared: Arc<Shared>) -> Box<dyn Editor> {
    Box::new(SlintEditor { params, shared, scale: Arc::new(AtomicU32::new(0)) })
}
struct SlintEditor {
    params: Arc<MelodyParams>,
    shared: Arc<Shared>,
    /// Host DPI factor. `0` means "not reported yet".
    scale: Arc<AtomicU32>,
}

fn global_settings(
    p: &MelodyParams,
    scale: f32,
    midi_dir: &Mutex<String>,
) -> globals::GlobalSettings {
    globals::GlobalSettings {
        theme: Some(p.theme.value().to_index()),
        scale: if scale > 0.0 { Some(scale) } else { None },
        midi_dir: midi_dir.lock().ok().map(|d| d.clone()).filter(|d| !d.trim().is_empty()),
    }
}
struct SlintEditorHandle {
    window: WindowHandle,
    params: Arc<MelodyParams>,
    shared: Arc<Shared>,
    scale: Arc<AtomicU32>,
    midi_dir: Arc<Mutex<String>>,
}
unsafe impl Send for SlintEditorHandle {}
impl Drop for SlintEditorHandle {
    fn drop(&mut self) {
        // Closing is the last chance to persist; read live state, not a spawn-time snapshot.
        globals::save_now(&global_settings(
            &self.params,
            f32::from_bits(self.scale.load(Relaxed)),
            &self.midi_dir,
        ));
        self.shared.text_focus.store(false, Relaxed);
        if let Ok(mut queue) = self.shared.key_queue.lock() { queue.clear(); }
        self.window.close();
    }
}

fn set<P: Param>(ctx: &Arc<dyn GuiContext>, param: &P, value: P::Plain) {
    let setter = ParamSetter::new(ctx.as_ref());
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// How many generators can use at most, for the current mode and cell count.
fn pulse_limit(mode: Mode, cells: i32) -> i32 {
    match mode {
        Mode::Dice => 3,
        Mode::Cellular => 15,
        Mode::Sieve => 2,
        Mode::AcidMotif => 16,
        Mode::Arpeggiator | Mode::WalkingBass => 4,
        Mode::HilbertCurve => 5,
        _ => cells,
    }
}

/// One mode-selection path for the list and the mode-header wheel.
fn apply_mode(c: &Arc<dyn GuiContext>, p: &MelodyParams, index: i32) {
    let last = <Mode as Enum>::variants().len() as i32 - 1;
    let mode = Mode::from_index(index.clamp(0, last) as usize);
    set(c, &p.mode, mode);
    let limit = pulse_limit(mode, p.cell_count());
    if p.pulses.value() > limit { set(c, &p.pulses, limit); }
}

/// Change clock rate and keep the history window inside the new pattern-length ceiling.
fn apply_rate(c: &Arc<dyn GuiContext>, p: &MelodyParams, rate: Rate) {
    set(c, &p.rate, rate);
    let max = crate::history_bars_ceiling(p.step_ceiling(), rate);
    if p.history_bars.value() > max { set(c, &p.history_bars, max); }
}

fn lfo_enabled_param(p: &MelodyParams, slot: i32) -> &BoolParam {
    match slot {
        1 => &p.lfo1_enabled,
        2 => &p.lfo2_enabled,
        3 => &p.lfo3_enabled,
        _ => &p.lfo4_enabled,
    }
}

fn lfo_wave_param(p: &MelodyParams, slot: i32) -> &EnumParam<OscWave> {
    match slot {
        1 => &p.lfo1_wave,
        2 => &p.lfo2_wave,
        3 => &p.lfo3_wave,
        _ => &p.lfo4_wave,
    }
}

fn lfo_rate_mode_param(p: &MelodyParams, slot: i32) -> &EnumParam<LfoRateMode> {
    match slot {
        1 => &p.lfo1_rate_mode,
        2 => &p.lfo2_rate_mode,
        3 => &p.lfo3_rate_mode,
        _ => &p.lfo4_rate_mode,
    }
}

fn lfo_freq_param(p: &MelodyParams, slot: i32) -> &FloatParam {
    match slot {
        1 => &p.lfo1_freq,
        2 => &p.lfo2_freq,
        3 => &p.lfo3_freq,
        _ => &p.lfo4_freq,
    }
}

fn lfo_beat_param(p: &MelodyParams, slot: i32) -> &FloatParam {
    match slot {
        1 => &p.lfo1_rate,
        2 => &p.lfo2_rate,
        3 => &p.lfo3_rate,
        _ => &p.lfo4_rate,
    }
}

fn lfo_depth_param(p: &MelodyParams, slot: i32) -> &FloatParam {
    match slot {
        1 => &p.lfo1_depth,
        2 => &p.lfo2_depth,
        3 => &p.lfo3_depth,
        _ => &p.lfo4_depth,
    }
}

fn lfo_phase_param(p: &MelodyParams, slot: i32) -> &FloatParam {
    match slot {
        1 => &p.lfo1_phase,
        2 => &p.lfo2_phase,
        3 => &p.lfo3_phase,
        _ => &p.lfo4_phase,
    }
}

fn lfo_polarity_param(p: &MelodyParams, slot: i32) -> &EnumParam<LfoPolarity> {
    match slot {
        1 => &p.lfo1_polarity,
        2 => &p.lfo2_polarity,
        3 => &p.lfo3_polarity,
        _ => &p.lfo4_polarity,
    }
}

/// Slider and spin-box paths share this so clamping stays identical.
fn set_opt(c: &Arc<dyn GuiContext>, p: &MelodyParams, key: i32, value: f32) {
    let snap = p.round_steps.value();
    // Host echo is asynchronous; use the value being set so dependent limits cannot lag a tick.
    if key == 14 {
        let cap = (value.round() as i32).clamp(MIN_MAX_STEPS as i32, MAX_STEPS as i32);
        set(c, &p.max_steps, cap);
        let cells = cell_count_for(p.steps.value(), snap, cap as usize);
        if p.steps.value() > cap as f32 { set(c, &p.steps, cap as f32); }
        let limit = pulse_limit(p.mode.value(), cells);
        if p.pulses.value() > limit { set(c, &p.pulses, limit); }
        if p.rotate.value() >= cells { set(c, &p.rotate, cells - 1); }
        let history_max = crate::history_bars_ceiling(cap as usize, p.rate.value());
        if p.history_bars.value() > history_max { set(c, &p.history_bars, history_max); }
        return;
    }
    let cap = p.step_ceiling();
    let cells = if key == 0 { cell_count_for(value, snap, cap) } else { p.cell_count() };
    let max_pulses = pulse_limit(p.mode.value(), cells);
    match key {
        0 => {
            let v = value.clamp(1.0, cap as f32);
            set(c, &p.steps, if snap { v.round() } else { v });
            if p.pulses.value() > max_pulses { set(c, &p.pulses, max_pulses); }
            if p.rotate.value() >= cells { set(c, &p.rotate, cells - 1); }
        }
        1 => set(c, &p.pulses, (value.round() as i32).clamp(0, max_pulses)),
        2 => set(c, &p.rotate, (value.round() as i32).clamp(0, cells - 1)),
        3 => set(c, &p.density, value.clamp(0.0, 1.0)),
        4 => set(c, &p.seed, value.clamp(0.0, 1.0)),
        5 => set(c, &p.phase, value.clamp(0.0, 1.0)),
        6 => set(c, &p.osc_rate, value.clamp(0.125, 8.0)),
        7 => set(c, &p.ca_rule, (value.round() as i32).clamp(0, 255)),
        8 => set(c, &p.probability, value.clamp(0.0, 1.0)),
        9 => set(c, &p.velocity, value.clamp(0.01, 1.0)),
        10 => set(c, &p.regenerate_every, (value.round() as i32).clamp(0, 256)),
        11 => set(c, &p.symmetry, (value.round() as i32).clamp(1, 8)),
        12 => set(c, &p.randomize_after_lengths, (value.round() as i32).clamp(0, 256)),
        13 => set(c, &p.swing, value.clamp(0.0, 0.5)),
        15 => set(c, &p.slide, value.clamp(0.0, 1.0)),
        16 => set(c, &p.accent, value.clamp(0.0, 1.0)),
        17 => set(c, &p.octave, value.clamp(0.0, 1.0)),
        18 => set(c, &p.root, (value.round() as i32).clamp(0, 11)),
        19 => set(c, &p.octaves, (value.round() as i32).clamp(1, 6)),
        20 => set(c, &p.midi_root, (value.round() as i32).clamp(0, 120)),
        21 => set(c, &p.edo, (value.round() as i32).clamp(1, crate::tuning::MAX_EDO as i32)),
        22 => set(c, &p.root_cents, value.clamp(-50.0, 50.0)),
        23 => set(c, &p.history_bars, value.clamp(1.0, p.history_bars_ceiling())),
        24 => set(c, &p.gate, value.clamp(0.02, 1.0)),
        25 => set(c, &p.bend_range, (value.round() as i32).clamp(1, 24)),
        _ => {},
    }
}

struct Cache {
    gen_key: Option<GenParams>,
    pattern: Pattern,
    /// Everything `view::build` reads besides the cached pattern.
    view_key: Option<(GenParams, i32, i32, usize)>,
}

fn rgb(c: [u8; 3]) -> slint::Color { slint::Color::from_rgb_u8(c[0], c[1], c[2]) }

static RANDOMIZE_NONCE: AtomicU32 = AtomicU32::new(1);
static MOD_REG_SEQ: AtomicU32 = AtomicU32::new(1);

/// One modulation drop target's window-space rectangle, reported by the Slint controls.
#[derive(Clone, Copy)]
struct ModRect {
    target: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    seq: u32,
}

fn scope_enabled(p: &MelodyParams, bit: i32) -> bool { p.randomize_scope.value() & bit != 0 }

/// Randomize through nice-plug's parameter setter, so host, UI and generator agree.
fn randomize_parameters(p: &MelodyParams, ctx: &Arc<dyn GuiContext>) {
    let nonce = RANDOMIZE_NONCE.fetch_add(1, Relaxed);
    let variation = p.variation.fetch_add(1, Relaxed).wrapping_add(nonce);
    let mut rng = Rng::new(p.seed.value(), variation, 0xA1_60_D2_0A_5EED);

    // Draw the whole set in fixed order, then apply only enabled groups.
    let mode = Mode::from_index(rng.below(<Mode as Enum>::variants().len()));
    let scale = Tuning::from_index(rng.below(<Tuning as Enum>::variants().len()));
    let ceiling = p.step_ceiling();
    let cells = 8 + rng.below(ceiling.saturating_sub(7).max(1));
    let steps = cells as f32;
    let mode_after = if scope_enabled(p, scope::MODE) { mode } else { p.mode.value() };
    let cells_after = if scope_enabled(p, scope::LENGTH) { cells } else { p.cell_count().max(1) as usize };
    let pulses = rng.below(pulse_limit(mode_after, cells_after as i32).max(0) as usize + 1) as i32;
    let rotate = rng.below(cells_after.max(1)) as i32;
    let density = 0.15 + 0.85 * rng.unit();
    let seed = rng.unit();
    let phase = rng.unit();
    let osc_rate = 0.125 + (8.0 - 0.125) * rng.unit();
    let wave = OscWave::from_index(rng.below(<OscWave as Enum>::variants().len()));
    let ca_rule = rng.below(256) as i32;
    let symmetry = 1 + rng.below(8) as i32;
    let probability = 0.35 + 0.65 * rng.unit();
    let velocity = 0.45 + 0.55 * rng.unit();
    let rate = Rate::from_index(rng.below(<Rate as Enum>::variants().len()));
    let root = rng.below(12) as i32;
    let octaves = 1 + rng.below(6) as i32;
    let edo = 1 + rng.below(crate::tuning::MAX_EDO as usize) as i32;
    let root_cents = -50.0 + 100.0 * rng.unit();
    let slide = rng.unit();
    let accent = rng.unit();
    let octave = rng.unit();
    let swing = 0.45 * rng.unit();
    let nts = rng.unit() >= 0.25;

    if scope_enabled(p, scope::MODE) { set(ctx, &p.mode, mode); }
    if scope_enabled(p, scope::LENGTH) { set(ctx, &p.steps, steps); }
    if scope_enabled(p, scope::HITS) {
        set(ctx, &p.pulses, pulses);
        set(ctx, &p.rotate, rotate);
        set(ctx, &p.density, density.clamp(0.0, 1.0));
    }
    if scope_enabled(p, scope::TUNING) {
        set(ctx, &p.scale, scale);
        set(ctx, &p.root, root);
        set(ctx, &p.octaves, octaves);
        set(ctx, &p.edo, edo);
        set(ctx, &p.root_cents, root_cents);
        // NTS gates every microtonal bend, so it belongs with tuning, not the feel group.
        set(ctx, &p.nts, nts);
    }
    if scope_enabled(p, scope::CLOCK) {
        set(ctx, &p.rate, rate);
        let history_max = crate::history_bars_ceiling(p.step_ceiling(), rate);
        if p.history_bars.value() > history_max { set(ctx, &p.history_bars, history_max); }
    }
    if scope_enabled(p, scope::PHASE) {
        set(ctx, &p.seed, seed);
        set(ctx, &p.phase, phase);
        set(ctx, &p.osc_rate, osc_rate);
        set(ctx, &p.osc_wave, wave);
        set(ctx, &p.ca_rule, ca_rule);
        set(ctx, &p.symmetry, symmetry);
    }
    if scope_enabled(p, scope::ACID) {
        set(ctx, &p.slide, slide);
        set(ctx, &p.accent, accent);
        set(ctx, &p.octave, octave);
    }
    if scope_enabled(p, scope::FEEL) {
        set(ctx, &p.probability, probability.clamp(0.0, 1.0));
        set(ctx, &p.velocity, velocity.clamp(0.01, 1.0));
        set(ctx, &p.swing, swing.clamp(0.0, 0.5));
    }
}

/// Pretty note name for the history header.
fn note_label(n: i32) -> String {
    format!("{}{}", NOTE_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

/// Write one generated loop to `folder` and return the file path. Editor thread only.
fn write_midi_file(p: &MelodyParams, bpm: f64, folder: &str) -> Result<PathBuf, String> {
    let folder = folder.trim();
    if folder.is_empty() {
        return Err("set an export folder first".to_string());
    }
    let folder = PathBuf::from(folder);
    std::fs::create_dir_all(&folder).map_err(|e| format!("create {}: {e}", folder.display()))?;

    let gen = p.gen_params();
    let pattern = generate(&gen);
    let settings = midi_export::ExportSettings {
        bpm,
        steps_per_beat: p.rate.value().steps_per_beat(),
        loop_steps: gen.length as f64,
        gate: p.gate.value() as f64,
        swing: p.swing.value() as f64,
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = folder.join(format!("nelody-seq-{stamp}.mid"));
    std::fs::write(&path, midi_export::smf(&pattern, p, &settings))
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

fn open_folder(path: &Path) {
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

fn push_view(ui: &AppWindow, v: &View) {
    // Update rows in place when the count is unchanged; replacing the model breaks tweening.
    let model = ui.get_view_nodes();
    if model.row_count() == v.nodes.len() {
        for (i, n) in v.nodes.iter().enumerate() {
            let node = ViewNode {
                x: n.x, y: n.y, r: n.r, on: n.on, cur: n.cur, label: SharedString::from(n.label.as_str()),
            };
            if model.row_data(i).as_ref() != Some(&node) { model.set_row_data(i, node); }
        }
    } else {
        ui.set_view_nodes(ModelRc::new(VecModel::from(v.nodes.iter().map(|n| ViewNode {
            x: n.x, y: n.y, r: n.r, on: n.on, cur: n.cur, label: SharedString::from(n.label.as_str()),
        }).collect::<Vec<_>>())));
    }
    ui.set_view_edges(SharedString::from(v.edges.as_str()));
    ui.set_view_fills(SharedString::from(v.fills.as_str()));
    ui.set_view_solid(SharedString::from(v.solid.as_str()));
    ui.set_view_slid(SharedString::from(v.slid.as_str()));
    ui.set_view_jumped(SharedString::from(v.jumped.as_str()));
    ui.set_view_lines(SharedString::from(v.lines.as_str()));
    ui.set_view_hand(SharedString::from(v.hand.as_str()));
}

/// Convert a history length in 4/4 bars to the editor-side millisecond window.
fn history_window_ms(bars: f32, bpm: f32) -> u64 {
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
    ((bars.clamp(1.0, MAX_STEPS as f32) * BEATS_PER_BAR * 60_000.0 / bpm).round() as u64).max(250)
}

/// Push the right-to-left history, with the window in milliseconds derived from tempo and bar count.
fn push_history(
    ui: &AppWindow,
    shared: &Shared,
    bpm: f32,
    bars: f32,
) {
    let window_ms = history_window_ms(bars, bpm);
    let now_ms = shared.epoch.elapsed().as_millis() as u64;
    let mut events = shared.snapshot();
    let roll = history::build(&mut events, now_ms, window_ms, HISTORY_W, HISTORY_H);
    ui.set_history_bg(SharedString::from(roll.bg.as_str()));
    ui.set_history_grid(SharedString::from(roll.grid.as_str()));
    ui.set_history_notes(SharedString::from(roll.notes.as_str()));
    ui.set_history_range(SharedString::from(format!(
        "{} – {}",
        note_label(roll.lo),
        note_label(roll.hi - 1)
    )));
}

/// Compact source label for one matrix row.
fn lfo_source_label(p: &MelodyParams, slot: usize) -> String {
    let spec = p.lfo_spec(slot);
    let wave = <OscWave as Enum>::variants()[spec.wave.to_index()];
    let speed = match spec.rate_mode {
        LfoRateMode::Hz => crate::format_hz(spec.freq_hz),
        LfoRateMode::Beats => format!("{:.2} beats", spec.rate_beats),
    };
    let polarity = match spec.polarity {
        LfoPolarity::Bipolar => "±",
        LfoPolarity::Unipolar => "+",
    };
    if spec.enabled {
        format!("LFO {} · {wave} · {speed} · {polarity}", slot + 1)
    } else {
        format!("LFO {} · off", slot + 1)
    }
}

/// One row per LFO/target link; the matrix is built on the editor thread while open.
fn matrix_rows(p: &MelodyParams, clock: crate::modulation::LfoClock) -> Vec<ModRow> {
    let signals = p.lfo_signals(clock);
    let mut rows = Vec::new();
    for slot in 0..LFO_COUNT {
        let mask = p.lfo_link_mask(slot);
        if mask == 0 {
            continue;
        }
        let source = lfo_source_label(p, slot);
        for target in ModTarget::ALL {
            if !target_in_mask(mask, target) {
                continue;
            }
            let base = p.target_value(target, &[0.0; LFO_COUNT]);
            let value = p.target_value(target, &signals);
            rows.push(ModRow {
                lfo: slot as i32 + 1,
                target_id: target.index() as i32,
                source: SharedString::from(source.as_str()),
                target: SharedString::from(target.label()),
                base: SharedString::from(format_target_value(target, base)),
                value: SharedString::from(format_target_value(target, value)),
                delta: SharedString::from(format!("{:+.2}", value - base)),
            });
        }
    }
    rows
}

fn lfo_dot_y(p: &MelodyParams, slot: usize, phase: f32) -> f32 {
    let wave = p.lfo_spec(slot).wave;
    4.0 + (1.0 - crate::sequence::oscillator_value(wave.to_index(), phase)) * 32.0
}

/// Per-target overlay data used by the parameter strips: base/effective normalized positions
/// plus the compact hover text listing the linked LFOs and their contribution to the range.
fn target_views(p: &MelodyParams, clock: crate::modulation::LfoClock) -> Vec<ModTargetView> {
    let signals = p.lfo_signals(clock);
    ModTarget::ALL
        .iter()
        .map(|target| {
            let target = *target;
            let base = p.target_value(target, &[0.0; LFO_COUNT]);
            let value = p.target_value(target, &signals);
            let (min, max) = p.target_range(target);
            let span = (max - min).max(1e-9);
            let base01 = ((base - min) / span).clamp(0.0, 1.0);
            let value01 = ((value - min) / span).clamp(0.0, 1.0);
            let mut parts = Vec::new();
            let mut depth_up = 0.0f32;
            let mut depth_down = 0.0f32;
            for slot in 0..LFO_COUNT {
                if !target_in_mask(p.lfo_link_mask(slot), target) {
                    continue;
                }
                let spec = p.lfo_spec(slot);
                if !spec.enabled {
                    parts.push(format!("LFO{} off", slot + 1));
                    continue;
                }
                let depth = spec.depth.clamp(0.0, 1.0) * 0.5;
                depth_up += depth;
                if spec.polarity == LfoPolarity::Bipolar {
                    depth_down += depth;
                }
                let speed = match spec.rate_mode {
                    LfoRateMode::Hz if spec.freq_hz >= 10.0 => {
                        format!("{:.0}Hz", spec.freq_hz)
                    }
                    LfoRateMode::Hz if spec.freq_hz >= 1.0 => format!("{:.1}Hz", spec.freq_hz),
                    LfoRateMode::Hz => format!("{:.2}Hz", spec.freq_hz),
                    LfoRateMode::Beats => format!("{:.1}b", spec.rate_beats),
                };
                // Half the range is full depth, so signal × 0.5 is the signed share.
                let percent = signals[slot] * 50.0;
                parts.push(format!(
                    "LFO{} {speed} {}{:.0}%",
                    slot + 1,
                    if percent >= 0.0 { "+" } else { "" },
                    percent
                ));
            }
            let lfo_text = parts.join(" · ");
            ModTargetView {
                target: target.index() as i32,
                linked: !parts.is_empty(),
                base,
                value,
                min,
                max,
                base01,
                value01,
                depth_up: depth_up.min(1.0),
                depth_down: depth_down.min(1.0),
                lfo_text: SharedString::from(lfo_text),
                count: parts.len() as i32,
            }
        })
        .collect()
}

/// Newest visible control under the pointer. Registration order resolves overlapping controls
/// after a mode change reflows the parameter column.
fn mod_target_at(rects: &[ModRect], x: f32, y: f32) -> Option<i32> {
    rects
        .iter()
        .filter(|rect| {
            x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h
        })
        .max_by_key(|rect| rect.seq)
        .map(|rect| rect.target)
}

impl Editor for SlintEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
        // User-global preferences load before the host's project state is shown.
        let stored = globals::load();
        self.shared.text_focus.store(false, Relaxed);
        if let Ok(mut queue) = self.shared.key_queue.lock() { queue.clear(); }
        let scale = stored.scale.filter(|s| *s > 0.0).unwrap_or_else(|| f32::from_bits(self.scale.load(Relaxed)));
        let midi_dir = Arc::new(Mutex::new(
            stored
                .midi_dir
                .clone()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| {
                    std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()
                }),
        ));
        let setup_midi_dir = midi_dir.clone();
        let update_midi_dir = midi_dir.clone();
        let mod_rects = Arc::new(Mutex::new(Vec::<ModRect>::new()));
        if scale > 0.0 && self.scale.load(Relaxed) == 0 { self.scale.store(scale.to_bits(), Relaxed); }
        if let Some(theme) = stored.theme {
            let theme = PluginTheme::from_index(theme);
            let setter = ParamSetter::new(context.as_ref());
            setter.begin_set_parameter(&self.params.theme);
            setter.set_parameter(&self.params.theme, theme);
            setter.end_set_parameter(&self.params.theme);
        }
        let options = WindowOpenOptions {
            title: "nelody seq".to_string(), size: Size::new(WIDTH as f64, HEIGHT as f64),
            scale: if scale > 0.0 { WindowScalePolicy::ScaleFactor(scale as f64) } else { WindowScalePolicy::SystemScaleFactor },
            gl_config: Some(default_gl_config()),
        };
        let (setup_params, setup_ctx) = (self.params.clone(), context.clone());
        let setup_shared = self.shared.clone();
        let setup_scale = self.scale.clone();
        let setup_mod_rects = mod_rects.clone();
        let setup = move |h: &slint_baseview::SlintWindowHandler<AppWindow>, _w: &mut slint_baseview::Window| {
            let ui = h.component();
            let folder = setup_midi_dir.lock().map(|d| d.clone()).unwrap_or_default();
            ui.set_midi_folder(SharedString::from(folder.as_str()));
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_mode(move |v| apply_mode(&c, &p, v));
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_step_mode(move |delta| {
                    // step through the order the list is drawn in, not the raw host index
                    apply_mode(&c, &p, p.mode.value().step_in_display_order(delta).to_index() as i32);
                });
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_step_rate(move |delta| apply_rate(&c, &p, p.rate.value().stepped(delta)));
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_rate(move |v| {
                    let last = <Rate as Enum>::variants().len() as i32 - 1;
                    apply_rate(&c, &p, Rate::from_index(v.clamp(0, last) as usize));
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_rate_submitted(move |text| {
                    if let Some(rate) = Rate::parse(text.as_str()) { apply_rate(&c, &p, rate); }
                });
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_osc_wave(move |v| {
                    set(&c, &p.osc_wave, OscWave::from_index(v.clamp(0, OscWave::variants().len() as i32 - 1) as usize));
                });
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_enabled(move |slot, enabled| {
                    set(&c, lfo_enabled_param(&p, slot), enabled);
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_wave(move |slot, v| {
                    let wave = OscWave::from_index(
                        v.clamp(0, OscWave::variants().len() as i32 - 1) as usize,
                    );
                    set(&c, lfo_wave_param(&p, slot), wave);
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_rate_mode(move |slot, v| {
                    let mode = LfoRateMode::from_index(
                        v.clamp(0, LfoRateMode::variants().len() as i32 - 1) as usize,
                    );
                    set(&c, lfo_rate_mode_param(&p, slot), mode);
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_speed(move |slot, value| {
                    match lfo_rate_mode_param(&p, slot).value() {
                        LfoRateMode::Hz => set(&c, lfo_freq_param(&p, slot), value.clamp(0.01, LFO_MAX_HZ)),
                        LfoRateMode::Beats => set(&c, lfo_beat_param(&p, slot), value.clamp(0.25, 16.0)),
                    }
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_depth(move |slot, value| {
                    set(&c, lfo_depth_param(&p, slot), value.clamp(0.0, 1.0));
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_phase(move |slot, value| {
                    set(&c, lfo_phase_param(&p, slot), value.clamp(0.0, 1.0));
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_lfo_polarity(move |slot, v| {
                    let polarity = LfoPolarity::from_index(
                        v.clamp(0, LfoPolarity::variants().len() as i32 - 1) as usize,
                    );
                    set(&c, lfo_polarity_param(&p, slot), polarity);
                });
            }
            {
                let rects = setup_mod_rects.clone();
                ui.global::<ModRegistry>().on_register(move |target, visible, x, y, w, h| {
                    if target < 0 {
                        return;
                    }
                    let mut rects = rects.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    rects.retain(|rect| rect.target != target);
                    if visible {
                        rects.push(ModRect {
                            target,
                            x,
                            y,
                            w,
                            h,
                            seq: MOD_REG_SEQ.fetch_add(1, Relaxed),
                        });
                    }
                });
            }
            {
                let (rects, p, c, weak) = (
                    setup_mod_rects.clone(),
                    setup_params.clone(),
                    setup_ctx.clone(),
                    ui.as_weak(),
                );
                ui.global::<ModDrag>().on_dropped(move |lfo, x, y| {
                    let target = {
                        let rects = rects.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        mod_target_at(&rects, x, y)
                    };
                    let Some(target) = target.and_then(|i| ModTarget::from_index(i.max(0) as usize))
                    else {
                        return;
                    };
                    let slot = lfo.round().clamp(1.0, 4.0) as usize;
                    let linked = target_in_mask(p.lfo_link_mask(slot - 1), target);
                    p.set_lfo_link(slot - 1, target, !linked);
                    // A new link supersedes the pre-matrix single target, including when the
                    // user removes the last new link.
                    if slot == 1 {
                        set(&c, &p.lfo1_target, LfoTarget::None);
                    } else if slot == 2 {
                        set(&c, &p.lfo2_target, LfoTarget::None);
                    }
                    if let Some(ui) = weak.upgrade() {
                        let action = if linked { "unlinked" } else { "linked" };
                        ui.set_mod_status(SharedString::from(format!(
                            "LFO {slot} → {} {action}",
                            target.label()
                        )));
                    }
                });
            }
            {
                let (p, c, weak) = (setup_params.clone(), setup_ctx.clone(), ui.as_weak());
                ui.on_delete_mod(move |lfo, target_index| {
                    let slot = lfo.clamp(1, 4) as usize;
                    let Some(target) = ModTarget::from_index(target_index.max(0) as usize) else {
                        return;
                    };
                    p.set_lfo_link(slot - 1, target, false);
                    if slot == 1 {
                        set(&c, &p.lfo1_target, LfoTarget::None);
                    } else if slot == 2 {
                        set(&c, &p.lfo2_target, LfoTarget::None);
                    }
                    if let Some(ui) = weak.upgrade() {
                        ui.set_mod_status(SharedString::from(format!(
                            "LFO {slot} → {} removed",
                            target.label()
                        )));
                    }
                });
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_scope(move |bit| {
                    let mask = p.randomize_scope.value() ^ (1 << bit.clamp(0, 7));
                    set(&c, &p.randomize_scope, mask);
                });
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_theme(move |v| set(&c, &p.theme, PluginTheme::from_index(v.max(0) as usize)));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_opt(move |key, value| set_opt(&c, &p, key, value));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_scale(move |v| {
                    set(&c, &p.scale, Tuning::from_index(v.max(0) as usize));
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_nts(move || set(&c, &p.nts, !p.nts.value()));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_free_run(move || set(&c, &p.free_run, !p.free_run.value()));
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_invert(move || set(&c, &p.invert, !p.invert.value()));
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_mirror(move || set(&c, &p.mirror, !p.mirror.value()));
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_logging(move || set(&c, &p.logging, !p.logging.value()));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                // Generate bumps `variation`; every generator now consumes it, so the Seed knob stays put.
                ui.on_generate_pressed(move || set(&c, &p.generate, !p.generate.value()));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_randomize_pressed(move || randomize_parameters(&p, &c));
            }
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_round_steps(move || set(&c, &p.round_steps, !p.round_steps.value()));
            }
            {
                let (p, dir, scale) = (setup_params.clone(), setup_midi_dir.clone(), setup_scale.clone());
                ui.on_set_midi_folder(move |value| {
                    let text = value.trim().to_string();
                    if let Ok(mut current) = dir.lock() { *current = text; }
                    globals::save(&global_settings(&p, f32::from_bits(scale.load(Relaxed)), &dir));
                });
            }
            {
                let (p, shared, dir) = (setup_params.clone(), setup_shared.clone(), setup_midi_dir.clone());
                let weak = ui.as_weak();
                ui.on_export_midi(move || {
                    let folder = dir.lock().map(|d| d.clone()).unwrap_or_default();
                    let bpm = match shared.tempo_bits.load(Relaxed) {
                        0 => 120.0,
                        bits => f32::from_bits(bits) as f64,
                    };
                    let status = match write_midi_file(&p, bpm, &folder) {
                        Ok(path) => {
                            open_folder(Path::new(&folder));
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            format!("Exported {name}")
                        }
                        Err(e) => format!("Export failed: {e}"),
                    };
                    if let Some(ui) = weak.upgrade() {
                        ui.set_export_status(SharedString::from(status));
                    }
                });
            }
            ui.set_wave_names(ModelRc::new(VecModel::from(
                OscWave::variants().iter().map(|name| SharedString::from(*name)).collect::<Vec<_>>(),
            )));
            ui.set_wave_paths(ModelRc::new(VecModel::from(
                view::wave_icon_paths().into_iter().map(SharedString::from).collect::<Vec<_>>(),
            )));
            ui.set_scale_names(ModelRc::new(VecModel::from(
                <Tuning as Enum>::variants().iter().map(|name| SharedString::from(*name)).collect::<Vec<_>>(),
            )));
            ui.set_rate_names(ModelRc::new(VecModel::from(
                <Rate as Enum>::variants().iter().map(|name| SharedString::from(*name)).collect::<Vec<_>>(),
            )));
            ui.set_mod_rows(ModelRc::new(VecModel::from(Vec::<ModRow>::new())));
            ui.set_mod_status(SharedString::new());
            ui.set_mode_rows(ModelRc::new(VecModel::from(mode_rows())));
            ui.set_app_version(SharedString::from(crate::contact::VERSION));
            ui.set_settings_path(SharedString::from(crate::globals::config_path_display()));
            ui.set_credit_author(SharedString::from(crate::contact::AUTHOR));
            ui.set_credit_github(SharedString::from(crate::contact::GITHUB));
            ui.set_credit_support(SharedString::from(crate::contact::SUPPORT));
            ui.set_credit_email(SharedString::from(crate::contact::EMAIL));
        };

        let cache = Arc::new(Mutex::new(Cache { gen_key: None, pattern: Pattern::default(), view_key: None }));
        let (update_params, update_shared, update_cache, update_context) =
            (self.params.clone(), self.shared.clone(), cache, context.clone());
        let update_scale_cell = self.scale.clone();
        let last_randomize_request = AtomicU32::new(update_shared.randomize_request.load(Relaxed));
        let last_regen_pulse = AtomicU32::new(update_shared.regen_pulse.load(Relaxed));
        // `u32::MAX` forces the first tick to publish whatever track name is already known.
        let last_track_rev = AtomicU32::new(u32::MAX);
        let update = move |h: &slint_baseview::SlintWindowHandler<AppWindow>, _window: &mut slint_baseview::Window| {
            let requested = update_shared.randomize_request.load(Relaxed);
            let previous = last_randomize_request.swap(requested, Relaxed);
            let pending = requested.wrapping_sub(previous).min(16);
            if pending > 0 {
                for _ in 0..pending { randomize_parameters(&update_params, &update_context); }
            }
            let p = &update_params;
            let ui = h.component();
            let regen_pulse = update_shared.regen_pulse.load(Relaxed);
            if last_regen_pulse.swap(regen_pulse, Relaxed) != regen_pulse {
                ui.set_regen_pulse(true);
            }
            // Replay editing keys the host would otherwise have kept for itself.
            let forwarded = update_shared.key_queue.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
            for (key, down) in forwarded {
                let text: SharedString = key.slint_key().into();
                h.window().dispatch_event(if down { WindowEvent::KeyPressed { text } } else { WindowEvent::KeyReleased { text } });
            }
            let track_rev = update_shared.track_rev.load(Relaxed);
            if last_track_rev.swap(track_rev, Relaxed) != track_rev {
                let name = update_shared.track_name.lock().map(|n| n.clone()).unwrap_or_default();
                ui.set_track_name(SharedString::from(name.as_str()));
            }
            ui.set_mode(p.mode.value().to_index() as i32);
            let selected_theme = p.theme.value();
            let theme_index = selected_theme.to_index() as i32;
            let update_scale = f32::from_bits(update_scale_cell.load(Relaxed));
            if ui.get_theme_index() != theme_index {
                let colors = selected_theme.colors();
                let palette = ui.global::<Theme>();
                palette.set_c1(rgb(colors[0])); palette.set_c2(rgb(colors[1])); palette.set_c3(rgb(colors[2]));
                palette.set_hi(rgb(colors[3])); palette.set_bg1(rgb(colors[4])); palette.set_bg2(rgb(colors[5])); palette.set_bg3(rgb(colors[6]));
                palette.set_surface(rgb(colors[7])); palette.set_control(rgb(colors[8])); palette.set_control_hover(rgb(colors[9]));
                palette.set_text(rgb(colors[10])); palette.set_text_muted(rgb(colors[11])); palette.set_border(rgb(colors[12]));
                palette.set_cell_off(rgb(colors[13])); palette.set_on_accent(rgb(colors[14]));
                ui.set_theme_index(theme_index);
                globals::save(&global_settings(p, update_scale, &update_midi_dir));
            }
            let ceiling = p.step_ceiling() as f32;
            ui.set_max_steps(ceiling);
            // Show the value the generator will actually use, not one above a lowered ceiling.
            ui.set_steps(p.steps.value().min(ceiling));
            ui.set_round_steps(p.round_steps.value());
            ui.set_pulses(p.pulses.value() as f32);
            ui.set_rotate(p.rotate.value() as f32);
            ui.set_density(p.density.value());
            ui.set_probability(p.probability.value());
            ui.set_velocity(p.velocity.value());
            ui.set_gate(p.gate.value());
            ui.set_regenerate_every(p.regenerate_every.value() as f32);
            ui.set_randomize_after_lengths(p.randomize_after_lengths.value() as f32);
            ui.set_swing(p.swing.value());
            ui.set_scope_mode(scope_enabled(p, scope::MODE));
            ui.set_scope_length(scope_enabled(p, scope::LENGTH));
            ui.set_scope_hits(scope_enabled(p, scope::HITS));
            ui.set_scope_tuning(scope_enabled(p, scope::TUNING));
            ui.set_scope_clock(scope_enabled(p, scope::CLOCK));
            ui.set_scope_phase(scope_enabled(p, scope::PHASE));
            ui.set_scope_acid(scope_enabled(p, scope::ACID));
            ui.set_scope_feel(scope_enabled(p, scope::FEEL));
            ui.set_seed(p.seed.value());
            ui.set_phase(p.phase.value());
            ui.set_osc_rate(p.osc_rate.value());
            ui.set_osc_wave(p.osc_wave.value().to_index() as i32);
            ui.set_ca_rule(p.ca_rule.value() as f32);
            ui.set_symmetry(p.symmetry.value() as f32);
            ui.set_rate(p.rate.value().to_index() as i32);
            let live_clock = update_shared.lfo_clock();
            let live_signals = p.lfo_signals(live_clock);
            let lfo_running = update_shared.lfo_running.load(Relaxed);
            ui.set_lfo_running(lfo_running);
            let views = target_views(p, live_clock);
            let mod_model = ui.global::<ModView>().get_targets();
            if mod_model.row_count() == views.len() {
                for (i, view) in views.iter().enumerate() {
                    if mod_model.row_data(i).as_ref() != Some(view) {
                        mod_model.set_row_data(i, view.clone());
                    }
                }
            } else {
                ui.global::<ModView>().set_targets(ModelRc::new(VecModel::from(views)));
            }
            for slot in 0..LFO_COUNT {
                let phase = f32::from_bits(update_shared.lfo_phase[slot].load(Relaxed));
                let dot_y = lfo_dot_y(p, slot, phase);
                match slot {
                    0 => {
                        ui.set_lfo1_enabled(p.lfo1_enabled.value());
                        ui.set_lfo1_wave(p.lfo1_wave.value().to_index() as i32);
                        ui.set_lfo1_rate_mode(p.lfo1_rate_mode.value().to_index() as i32);
                        ui.set_lfo1_freq(p.lfo1_freq.value());
                        ui.set_lfo1_rate(p.lfo1_rate.value());
                        ui.set_lfo1_depth(p.lfo1_depth.value());
                        ui.set_lfo1_phase(p.lfo1_phase.value());
                        ui.set_lfo1_polarity(p.lfo1_polarity.value().to_index() as i32);
                        ui.set_lfo1_anim_phase(phase);
                        ui.set_lfo1_dot_y(dot_y);
                    }
                    1 => {
                        ui.set_lfo2_enabled(p.lfo2_enabled.value());
                        ui.set_lfo2_wave(p.lfo2_wave.value().to_index() as i32);
                        ui.set_lfo2_rate_mode(p.lfo2_rate_mode.value().to_index() as i32);
                        ui.set_lfo2_freq(p.lfo2_freq.value());
                        ui.set_lfo2_rate(p.lfo2_rate.value());
                        ui.set_lfo2_depth(p.lfo2_depth.value());
                        ui.set_lfo2_phase(p.lfo2_phase.value());
                        ui.set_lfo2_polarity(p.lfo2_polarity.value().to_index() as i32);
                        ui.set_lfo2_anim_phase(phase);
                        ui.set_lfo2_dot_y(dot_y);
                    }
                    2 => {
                        ui.set_lfo3_enabled(p.lfo3_enabled.value());
                        ui.set_lfo3_wave(p.lfo3_wave.value().to_index() as i32);
                        ui.set_lfo3_rate_mode(p.lfo3_rate_mode.value().to_index() as i32);
                        ui.set_lfo3_freq(p.lfo3_freq.value());
                        ui.set_lfo3_rate(p.lfo3_rate.value());
                        ui.set_lfo3_depth(p.lfo3_depth.value());
                        ui.set_lfo3_phase(p.lfo3_phase.value());
                        ui.set_lfo3_polarity(p.lfo3_polarity.value().to_index() as i32);
                        ui.set_lfo3_anim_phase(phase);
                        ui.set_lfo3_dot_y(dot_y);
                    }
                    _ => {
                        ui.set_lfo4_enabled(p.lfo4_enabled.value());
                        ui.set_lfo4_wave(p.lfo4_wave.value().to_index() as i32);
                        ui.set_lfo4_rate_mode(p.lfo4_rate_mode.value().to_index() as i32);
                        ui.set_lfo4_freq(p.lfo4_freq.value());
                        ui.set_lfo4_rate(p.lfo4_rate.value());
                        ui.set_lfo4_depth(p.lfo4_depth.value());
                        ui.set_lfo4_phase(p.lfo4_phase.value());
                        ui.set_lfo4_polarity(p.lfo4_polarity.value().to_index() as i32);
                        ui.set_lfo4_anim_phase(phase);
                        ui.set_lfo4_dot_y(dot_y);
                    }
                }
            }
            if ui.get_matrix_open() {
                let rows = matrix_rows(p, live_clock);
                let model = ui.get_mod_rows();
                if model.row_count() == rows.len() {
                    // Update in place; rebuilding every frame interrupts clicks on the delete button.
                    for (i, row) in rows.iter().enumerate() {
                        if model.row_data(i).as_ref() != Some(row) {
                            model.set_row_data(i, row.clone());
                        }
                    }
                } else {
                    ui.set_mod_rows(ModelRc::new(VecModel::from(rows)));
                }
            }
            ui.set_free_run(p.free_run.value());
            ui.set_invert_on(p.invert.value());
            ui.set_mirror_on(p.mirror.value());
            ui.set_logging_on(p.logging.value());
            ui.set_generate_on(p.generate.value());
            let scale_index = p.scale.value().to_index();
            ui.set_tuning_name(SharedString::from(<Tuning as Enum>::variants()[scale_index]));
            ui.set_scale_index(scale_index as i32);
            ui.set_root_pc(p.root.value());
            ui.set_octaves(p.octaves.value());
            ui.set_midi_root(p.midi_root.value());
            ui.set_edo(p.edo.value());
            ui.set_edo_active(p.scale.value() == Tuning::Edo);
            ui.set_root_cents(p.root_cents.value());
            ui.set_bend_range(p.bend_range.value() as f32);
            let history_max = p.history_bars_ceiling();
            let history_bars = p.history_bars.value().min(history_max);
            if p.history_bars.value() > history_max { set(&update_context, &p.history_bars, history_max); }
            ui.set_history_bars(history_bars);
            ui.set_history_max_bars(history_max);
            let history_bars_effective = p
                .target_value(ModTarget::HistoryBars, &live_signals)
                .clamp(1.0, history_max);
            ui.set_nts_on(p.nts.value());
            ui.set_slide(p.slide.value());
            ui.set_accent(p.accent.value());
            ui.set_octave(p.octave.value());

            let preview_clock = update_shared.pattern_clock();
            let gen_key = p.gen_params_at(preview_clock);
            let preview_pitch = p.pitch_values_at(preview_clock);
            let cur = update_shared.current_step.load(Relaxed);
            ui.set_current_step(cur);
            let mut c = update_cache.lock().unwrap_or_else(|poisoned| {
                log_line("WARNING: update cache mutex was poisoned; recovering its inner state");
                poisoned.into_inner()
            });
            if c.gen_key != Some(gen_key) {
                c.pattern = generate(&gen_key);
                ui.set_mode_math(SharedString::from(crate::help::math(&gen_key)));
                if p.logging.value() {
                    log_line(&format!(
                        "mode={:?} steps={} density={:.2} probability={:.2} seed={:.3} rotate={} invert={} mirror={} root={} scale={:?} octaves={}",
                        gen_key.kind, gen_key.steps, gen_key.density, gen_key.probability,
                        gen_key.seed, gen_key.rotate, gen_key.invert, gen_key.mirror,
                        p.root.value(), p.scale.value(), p.octaves.value(),
                    ));
                }
                c.gen_key = Some(gen_key);
            }
            // Read after the replayed keys were dispatched, so the host hook sees fresh focus.
            let text_focused = ui.get_text_focused();
            update_shared.text_focus.store(text_focused, Relaxed);
            // Keep the host from acting on the same character keys Slint is inserting.
            h.set_prevent_key_event_propagation(text_focused);
            let view_key = (gen_key, cur, preview_pitch.root, gen_key.osc_wave);
            if c.view_key != Some(view_key) {
                let v = view::build(gen_key.kind, &c.pattern, c.pattern.steps, cur, preview_pitch.root, gen_key.osc_wave);
                push_view(&ui, &v);
                c.view_key = Some(view_key);
            }
            push_history(&ui, &update_shared, update_shared.sample_tempo(), history_bars_effective);
        };
        let factory = move || {
            let ui = AppWindow::new()?;
            Ok(ui)
        };
        log_line("editor spawn: opening parented Slint/baseview window");
        let window = SlintWindow::open_parented(&parent, options, factory, setup, update, |_, _| {});
        log_line("editor spawn: open_parented returned; native initialization may continue asynchronously");
        Box::new(SlintEditorHandle {
            window,
            params: self.params.clone(),
            shared: self.shared.clone(),
            scale: self.scale.clone(),
            midi_dir,
        })
    }
    fn size(&self) -> (u32, u32) { (WIDTH, HEIGHT) }
    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale.store(factor.to_bits(), Relaxed);
        let mut s = globals::load();
        s.scale = if factor > 0.0 { Some(factor) } else { None };
        globals::save(&s);
        true
    }
    /// Claim a host-stolen editing key only while a text field has focus.
    fn on_virtual_key_from_host(&self, key_code: VirtualKeyCode, is_down: bool, modifiers: Modifiers) -> bool {
        if !self.shared.text_focus.load(Relaxed) { return false; }
        let Some(key) = keys::edit_key(key_code, modifiers) else { return false };
        if let Ok(mut queue) = self.shared.key_queue.lock() {
            if queue.len() < 64 { queue.push((key, is_down)); }
        }
        true
    }
    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_mode_pulse_limits_stay_inside_the_grid() {
        assert_eq!(pulse_limit(Mode::Dice, 64), 3);
        assert_eq!(pulse_limit(Mode::Cellular, 64), 15);
        assert_eq!(pulse_limit(Mode::Sieve, 64), 2);
        assert_eq!(pulse_limit(Mode::AcidMotif, 64), 16);
        assert_eq!(pulse_limit(Mode::Euclidean, 16), 16);
    }

    #[test]
    fn grouped_mode_entries_start_with_headers_and_keep_host_indices() {
        let entries = mode_entries();
        assert!(entries[0].header, "the first row must be a group header");
        assert_eq!(entries[0].group.as_str(), "Rhythm");
        assert!(entries.iter().any(|e| e.index == Mode::HilbertCurve.to_index() as i32));
        // Every host index appears exactly once and all headers are in sorted group order.
        let mut indices: Vec<i32> = entries.iter().filter(|e| !e.header).map(|e| e.index).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..<Mode as Enum>::variants().len() as i32).collect::<Vec<_>>());
    }

    #[test]
    fn mode_rows_pair_two_modes_per_row_and_keep_every_host_index() {
        let rows = mode_rows();
        assert!(rows[0].header, "the first row must be a group header");
        assert_eq!(rows[0].group.as_str(), "Rhythm");
        let mut indices = Vec::new();
        for row in &rows {
            if row.header {
                assert_eq!((row.left, row.right, row.has_right), (-1, -1, false));
                continue;
            }
            assert!(row.left >= 0);
            indices.push(row.left);
            if row.has_right {
                assert!(row.right >= 0);
                indices.push(row.right);
            } else {
                assert_eq!(row.right, -1, "a one-column row must not name a right mode");
            }
        }
        indices.sort_unstable();
        assert_eq!(indices, (0..<Mode as Enum>::variants().len() as i32).collect::<Vec<_>>());
    }

    #[test]
    fn history_lengths_are_four_four_bars() {
        assert_eq!(history_window_ms(4.0, 120.0), 8_000);
        assert_eq!(history_window_ms(4.0, 60.0), 16_000);
        assert_eq!(history_window_ms(1.0, 0.0), 2_000);
    }

    #[test]
    fn history_range_labels_use_midi_note_names() {
        assert_eq!(note_label(48), "C3");
        assert_eq!(note_label(60), "C4");
        assert_eq!(note_label(61), "C#4");
        assert_eq!(note_label(0), "C-1");
    }

    #[test]
    fn modulation_hit_test_prefers_the_newest_overlapping_control() {
        let rects = [
            ModRect { target: 0, x: 0.0, y: 0.0, w: 100.0, h: 20.0, seq: 1 },
            ModRect { target: 3, x: 0.0, y: 0.0, w: 100.0, h: 20.0, seq: 2 },
        ];
        assert_eq!(mod_target_at(&rects, 50.0, 10.0), Some(3));
        assert_eq!(mod_target_at(&rects, 150.0, 10.0), None);
    }

    #[test]
    fn appwindow_startup_runs_timers_without_property_recursion() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use slint::platform::{Platform, WindowAdapter};
        use std::rc::Rc;
        use std::time::{Duration, Instant};

        struct TestPlatform {
            window: Rc<MinimalSoftwareWindow>,
            start: Instant,
        }
        impl Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(self.window.clone())
            }
            fn duration_since_start(&self) -> Duration {
                self.start.elapsed()
            }
        }

        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        let start = Instant::now();
        slint::platform::set_platform(Box::new(TestPlatform { window: window.clone(), start }))
            .unwrap();
        window.set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
        let ui = AppWindow::new().unwrap();
        let registered = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registered_in_cb = registered.clone();
        ui.global::<ModRegistry>().on_register(move |target, visible, _, _, _, _| {
            if target >= 0 && visible {
                registered_in_cb.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
        ui.global::<ModDrag>().on_dropped(|_, _, _| {});
        let mut views = target_views(
            &MelodyParams::default(),
            crate::modulation::LfoClock::default(),
        );
        views[ModTarget::Steps.index()].linked = true;
        views[ModTarget::Steps.index()].base = 16.0;
        views[ModTarget::Steps.index()].value = 20.0;
        views[ModTarget::Steps.index()].depth_up = 0.08;
        views[ModTarget::Steps.index()].depth_down = 0.08;
        views[ModTarget::Steps.index()].lfo_text = SharedString::from("LFO1 0.5Hz +20%");
        ui.global::<ModView>().set_targets(ModelRc::new(VecModel::from(views)));
        ui.show().unwrap();
        let _ = window.try_dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(450.0, 178.0),
        });
        let mut buffer = vec![slint::Rgb8Pixel::new(0, 0, 0); (WIDTH * HEIGHT) as usize];
        for _ in 0..40 {
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut buffer, WIDTH as usize);
            });
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(
            registered.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "modulation drop targets were never registered"
        );
        assert_eq!(
            ui.global::<ModHover>().get_target(),
            ModTarget::Steps.index() as i32,
            "hover timer did not report the parameter under the pointer"
        );
    }

    #[test]
    fn target_views_expose_base_and_modulated_positions() {
        let p = MelodyParams {
            lfo1_links: std::sync::atomic::AtomicU64::new(crate::modulation::target_bit(
                ModTarget::Steps,
            )),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: FloatParam::new(
                "LFO 1 Frequency",
                0.5,
                FloatRange::Skewed { min: 0.01, max: LFO_MAX_HZ, factor: 0.25 },
            ),
            lfo1_depth: FloatParam::new(
                "LFO 1 Depth",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),
            ..MelodyParams::default()
        };
        let views = target_views(
            &p,
            crate::modulation::LfoClock { beat: 0.5, seconds: 0.5 },
        );
        let steps = &views[ModTarget::Steps.index()];
        assert!(steps.linked);
        assert!(steps.value01 > steps.base01);
        assert!(steps.lfo_text.as_str().contains("LFO1"));
        assert!(!views[ModTarget::Density.index()].linked);
    }

    #[test]
    fn matrix_rows_follow_links_and_show_modulated_values() {
        let p = MelodyParams {
            lfo1_links: std::sync::atomic::AtomicU64::new(crate::modulation::target_bit(
                ModTarget::Steps,
            )),
            lfo1_wave: EnumParam::new("LFO 1 Wave", OscWave::Sine),
            lfo1_rate_mode: EnumParam::new("LFO 1 Rate Mode", LfoRateMode::Hz),
            lfo1_freq: FloatParam::new(
                "LFO 1 Frequency",
                0.5,
                FloatRange::Skewed { min: 0.01, max: LFO_MAX_HZ, factor: 0.25 },
            ),
            lfo1_depth: FloatParam::new(
                "LFO 1 Depth",
                1.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),
            ..MelodyParams::default()
        };
        let rows = matrix_rows(&p, crate::modulation::LfoClock { beat: 0.5, seconds: 0.5 });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target.as_str(), "Steps");
        assert_eq!(rows[0].target_id, ModTarget::Steps.index() as i32);
        assert_ne!(rows[0].value.as_str(), rows[0].base.as_str());
    }
}
