//! Slint GUI adapter between pure geometry and UI glue. Everything here runs on the editor
//! thread; `process()` never sees Slint and the editor regenerates only when a snapshot changes.
use crate::edits::{
    quantize_length, EditState, ACCENT_AUTO, HIT_AUTO, HIT_OFF, HIT_ON, PROB_AUTO,
};
use crate::globals;
use crate::history;
use crate::keys;
use crate::midi_export;
use crate::sequence::{generate_with, GenParams, Pattern, Rng, MAX_STEPS, MAX_VOICES, MIN_MAX_STEPS};
use crate::view::{self, View};
use crate::modulation::{format_target_value, target_in_mask, LFO_COUNT, LFO_MAX_HZ};
use crate::{
    cell_count_for, scope, LfoPolarity, LfoRateMode, LfoTarget, MelodyParams, Mode, ModTarget,
    OscWave, Shared, Theme as PluginTheme, Tuning, VoiceMode,
};
use nice_plug::prelude::*;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use slint_baseview::{default_gl_config, Size, SlintWindow, WindowHandle, WindowOpenOptions, WindowScalePolicy};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

slint::include_modules!();
const WIDTH: u32 = 860;
const HEIGHT: u32 = 697;
/// Drawing size of the history roll, matching the Slint path viewbox.
const HISTORY_W: f32 = 800.0;
const HISTORY_H: f32 = 200.0;
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Built-in starting points. These are editor conveniences, not host parameters: applying one
/// writes ordinary host parameters so the result remains automatable and saveable.
struct Preset {
    name: &'static str,
    mode: Mode,
    steps: f32,
    pulses: i32,
    density: f32,
    probability: f32,
    seed: f32,
}

const PRESETS: &[Preset] = &[
    Preset { name: "ACID", mode: Mode::AcidEuclid, steps: 16.0, pulses: 5, density: 0.82, probability: 1.0, seed: 0.271 },
    Preset { name: "ARP", mode: Mode::Arpeggiator, steps: 16.0, pulses: 4, density: 0.88, probability: 1.0, seed: 0.413 },
    Preset { name: "BASS", mode: Mode::WalkingBass, steps: 16.0, pulses: 4, density: 0.90, probability: 1.0, seed: 0.127 },
    Preset { name: "DICE", mode: Mode::Dice, steps: 32.0, pulses: 2, density: 0.70, probability: 0.95, seed: 0.611 },
    Preset { name: "AMBIENT", mode: Mode::LSystem, steps: 64.0, pulses: 8, density: 0.55, probability: 0.90, seed: 0.337 },
];

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
        // Rounded out from disk so a theme change cannot reset the motion preference.
        reduce_motion: globals::load().reduce_motion,
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

/// Editor-local undo stack for the per-step overlay. Bounded so a long editing session cannot
/// grow without limit; pattern changes are not recorded here (the host owns parameter undo).
#[derive(Default)]
struct EditHistory {
    undo: Vec<EditState>,
    redo: Vec<EditState>,
}

impl EditHistory {
    const LIMIT: usize = 64;

    fn record(&mut self, state: EditState) {
        if self.undo.last().map(|last| &last.cells) == Some(&state.cells) { return; }
        self.undo.push(state);
        if self.undo.len() > Self::LIMIT { self.undo.remove(0); }
        self.redo.clear();
    }

    fn undo(&mut self, current: EditState) -> Option<EditState> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }

    fn redo(&mut self, current: EditState) -> Option<EditState> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}

/// Apply one edit-mode click to a step. Modes match the UI strip: HIT, PROB, ACC, CLR, RAT, TIME.
fn edit_step(p: &MelodyParams, step: usize, mode: i32) {
    let store = &p.edit_overlay;
    let step = step.min((p.cell_count().max(1) - 1) as usize);
    match mode {
        1 => store.set_hit(step, match store.hit(step) {
            HIT_AUTO => HIT_ON,
            HIT_ON => HIT_OFF,
            _ => HIT_AUTO,
        }),
        2 => store.set_probability(step, match store.probability(step) {
            255 => 254,
            254 => 191,
            191 => 127,
            127 => 64,
            _ => 255,
        }),
        3 => store.set_accent(step, match store.accent(step) {
            0 => 255,
            255 => 191,
            191 => 128,
            _ => 0,
        }),
        4 => store.clear_cell(step),
        5 => store.set_ratchet(step, match store.ratchet(step) {
            1 => 2,
            2 => 3,
            3 => 4,
            _ => 1,
        }),
        6 => store.set_offset(step, match store.offset(step).max(0) {
            0 => 16,
            16 => 32,
            32 => 64,
            _ => 0,
        }),
        _ => {}
    }
}

/// One-line description of every override on a step, shown while editing.
fn edit_status(p: &MelodyParams, step: usize) -> String {
    let store = &p.edit_overlay;
    let step = step.min(p.cell_count().saturating_sub(1) as usize);
    let hit = match store.hit(step) {
        HIT_AUTO => "auto",
        HIT_ON => "on",
        _ => "off",
    };
    let probability = store.probability(step);
    let probability = if probability == PROB_AUTO {
        "auto".to_string()
    } else {
        format!("{}%", (probability as f32 / 254.0 * 100.0).round() as i32)
    };
    let accent = store.accent(step);
    let accent = if accent == ACCENT_AUTO {
        "auto".to_string()
    } else {
        format!("{}%", (accent as f32 / 255.0 * 100.0).round() as i32)
    };
    let time = match store.offset(step).max(0) {
        0 => "0",
        16 => "1/8",
        32 => "1/4",
        64 => "1/2",
        _ => "custom",
    };
    format!(
        "STEP {} · HIT {} · PROB {} · ACC {} · RAT {} · TIME {}",
        step + 1,
        hit,
        probability,
        accent,
        store.ratchet(step),
        time
    )
}

/// Map a click in the 0..1 width of the history roll to the current pattern's display step.
/// The roll flows right to left, so x = 1 is the newest beat and x = 0 the oldest edge of the
/// visible window. Swing is ignored for hit-testing; it moves onsets by a fraction of a step.
fn overlay_position(
    p: &MelodyParams,
    shared: &Shared,
    bpm: f32,
    now_ms: u64,
    x01: f32,
) -> Option<(usize, f32)> {
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
    let spb = p.steps_per_beat();
    let step_ms = 60_000.0 / (bpm as f64 * spb);
    let phase_steps = roll_phase_steps(shared, bpm, now_ms, spb);
    let window_ms = history_window_ms(p.history_bars.value(), bpm, p.steps_per_beat(), p.loop_steps());
    let age_steps = (1.0f64 - f64::from(x01.clamp(0.0, 1.0))) * window_ms as f64 / step_ms;
    let k = phase_steps - age_steps;
    let gen = p.gen_params();
    let loop_len = gen.steps.max(1) as f64;
    let wrapped = k.rem_euclid(loop_len);
    let step = (wrapped.floor() as usize).min(gen.steps.saturating_sub(1));
    Some((step, (wrapped - wrapped.floor()) as f32))
}

/// Current step phase for the roll. Before the first processed block there is no scheduler
/// anchor, so the editor uses wall-clock time; this keeps the current-pattern ghost scrolling in
/// lockstep with the recorded history instead of freezing at beat zero.
fn roll_phase_steps(shared: &Shared, bpm: f32, now_ms: u64, steps_per_beat: f64) -> f64 {
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm as f64 } else { 120.0 };
    let beat = f64::from_bits(shared.last_beat.load(Relaxed));
    let beat_ms = shared.beat_ms.load(Relaxed) as f64;
    let now_beat = if beat_ms > 0.0 {
        beat + (now_ms as f64 - beat_ms) * bpm / 60_000.0
    } else {
        now_ms as f64 * bpm / 60_000.0
    };
    now_beat * steps_per_beat
}

/// Map a click in the 0..1 width of the history roll to a display step.
fn overlay_step_at(
    p: &MelodyParams,
    shared: &Shared,
    bpm: f32,
    now_ms: u64,
    x01: f32,
) -> Option<usize> {
    overlay_position(p, shared, bpm, now_ms, x01).map(|(step, _)| step)
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

/// Raise the loop and ceiling to a mode's step floor, then re-clamp everything that depends on
/// the loop length. Never lowers a value; only user-initiated mode changes call this.
fn apply_mode_shape(c: &Arc<dyn GuiContext>, p: &MelodyParams, mode: Mode, steps: f32) {
    let (steps, ceiling) = mode.steps_for_mode(steps, p.step_ceiling());
    if ceiling != p.step_ceiling() { set(c, &p.max_steps, ceiling as i32); }
    if (steps - p.steps.value()).abs() > 1e-6 { set(c, &p.steps, steps); }
    let cells = cell_count_for(steps, p.round_steps.value(), ceiling);
    let limit = pulse_limit(mode, cells);
    if p.pulses.value() > limit { set(c, &p.pulses, limit); }
    if p.rotate.value() >= cells { set(c, &p.rotate, cells - 1); }
    let history_max = crate::history_bars_ceiling(ceiling, cells as u32);
    if p.history_bars.value() > history_max { set(c, &p.history_bars, history_max); }
}

fn apply_preset(c: &Arc<dyn GuiContext>, p: &MelodyParams, index: i32) {
    let Some(preset) = PRESETS.get(index.max(0) as usize) else { return; };
    set(c, &p.mode, preset.mode);
    // The preset's loop is a suggestion; the mode's functionality floor still wins.
    apply_mode_shape(c, p, preset.mode, preset.steps);
    let (steps, ceiling) = preset.mode.steps_for_mode(preset.steps, p.step_ceiling());
    let cells = cell_count_for(steps, p.round_steps.value(), ceiling);
    set(c, &p.pulses, preset.pulses.min(pulse_limit(preset.mode, cells)));
    set(c, &p.density, preset.density);
    set(c, &p.probability, preset.probability);
    set(c, &p.seed, preset.seed);
    p.variation.fetch_add(1, Relaxed);
}

/// One mode-selection path for the list and the mode-header wheel.
fn apply_mode(c: &Arc<dyn GuiContext>, p: &MelodyParams, index: i32) {
    let last = <Mode as Enum>::variants().len() as i32 - 1;
    let mode = Mode::from_index(index.clamp(0, last) as usize);
    set(c, &p.mode, mode);
    apply_mode_shape(c, p, mode, p.steps.value());
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
        let history_max = crate::history_bars_ceiling(cap as usize, cells as u32);
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
        26 => set(c, &p.voices, (value.round() as i32).clamp(1, MAX_VOICES as i32)),
        27 => set(c, &p.voice_spread, value.clamp(0.0, 1.0)),
        28 => set(c, &p.voice_mode, VoiceMode::from_index(value.max(0.0) as usize)),
        29 => set(c, &p.voice_preview, (value.round() as i32).clamp(0, MAX_VOICES as i32 - 1)),
        _ => {},
    }
}

struct Cache {
    gen_key: Option<GenParams>,
    pattern: Pattern,
    voice_preview: usize,
    /// Everything `view::build` reads besides the cached pattern.
    view_key: Option<(GenParams, i32, i32, usize, usize)>,
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
    let cells = crate::randomize::length(ceiling, &mut rng);
    let steps = cells as f32;
    let mode_after = if scope_enabled(p, scope::MODE) { mode } else { p.mode.value() };
    let cells_after = if scope_enabled(p, scope::LENGTH) { cells } else { p.cell_count().max(1) as usize };
    let pulses = crate::randomize::pulses(
        mode_after,
        cells_after,
        pulse_limit(mode_after, cells_after as i32).max(1) as usize,
        &mut rng,
    ) as i32;
    let rotate = rng.below(cells_after.max(1)) as i32;
    let density = crate::randomize::pick(crate::randomize::DENSITIES, &mut rng);
    let seed = rng.unit();
    let phase = rng.unit();
    let osc_rate = 0.125 + (8.0 - 0.125) * rng.unit();
    let wave = OscWave::from_index(rng.below(<OscWave as Enum>::variants().len()));
    let ca_rule = rng.below(256) as i32;
    let symmetry = 1 + rng.below(8) as i32;
    let probability = crate::randomize::pick(crate::randomize::PROBABILITIES, &mut rng);
    let velocity = crate::randomize::pick(crate::randomize::VELOCITIES, &mut rng);
    let root = crate::randomize::pick(crate::randomize::ROOTS, &mut rng);
    let octaves = crate::randomize::pick(crate::randomize::OCTAVES, &mut rng);
    let edo = crate::randomize::pick(crate::randomize::EDOS, &mut rng);
    let root_cents = -50.0 + 100.0 * rng.unit();
    let slide = rng.unit();
    let accent = rng.unit();
    let octave = rng.unit();
    let swing = crate::randomize::pick(crate::randomize::SWINGS, &mut rng);
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
    // A randomized mode still needs its functionality floor; the drawn length is kept when it
    // is already long enough.
    if scope_enabled(p, scope::MODE) || scope_enabled(p, scope::LENGTH) {
        let steps = if scope_enabled(p, scope::LENGTH) { steps } else { p.steps.value() };
        apply_mode_shape(ctx, p, mode_after, steps);
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
    let mut edit_view = crate::edits::EditView::default();
    p.edit_overlay.fill(&mut edit_view);
    let voices: Vec<Pattern> = (0..p.voice_count())
        .map(|voice| generate_with(&p.voice_params(gen, voice), &edit_view))
        .collect();
    let same_loop = voices.windows(2).all(|w| w[0].steps == w[1].steps);
    let settings = midi_export::ExportSettings {
        bpm,
        steps_per_beat: p.steps_per_beat(),
        loop_steps: p.loop_steps(),
        meter: if same_loop {
            crate::clock::meter_from_steps(voices[0].steps.max(1) as u32, p.step_unit())
        } else {
            None
        },
        gate: p.gate.value() as f64,
        swing: p.swing.value() as f64,
        offsets: std::array::from_fn(|step| edit_view.offset_at(step).max(0.0)),
        ratchets: std::array::from_fn(|step| edit_view.ratchet_at(step)),
        lengths: std::array::from_fn(|step| edit_view.length_at(step).unwrap_or(0.0)),
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = folder.join(format!("nelody-seq-{stamp}.mid"));
    std::fs::write(&path, midi_export::smf_voices(&voices, p, &settings))
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

/// Convert a history length in derived pattern loops to the editor-side millisecond window.
fn history_window_ms(bars: f32, bpm: f32, steps_per_beat: f64, loop_steps: u32) -> u64 {
    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
    let spb = if steps_per_beat.is_finite() && steps_per_beat > 0.0 { steps_per_beat } else { 4.0 };
    let loop_len = loop_steps.max(1) as f64;
    let step_ms = 60_000.0 / (bpm as f64 * spb);
    ((bars.clamp(1.0, MAX_STEPS as f32) as f64 * loop_len * step_ms).round() as u64).max(250)
}

/// Note bars for the current-loop overlay, built through the same `pitch_for` path as playback.
fn overlay_notes(p: &MelodyParams, pattern: &Pattern, pitch: &crate::PitchValues) -> Vec<history::Note> {
    let gate = p.gate.value().clamp(0.02, 1.0);
    let n = pattern.steps;
    let nts = p.nts.value();
    let mut out = Vec::new();
    for i in 0..n {
        if !pattern.hit[i] { continue; }
        let (note, cents) = p.pitch_for_values(pattern, i, pitch);
        // Same hold rule as playback and export: a slide runs to the next onset, else the
        // per-step length override, else the global gate.
        let length = p.edit_overlay.length_at(i).unwrap_or(gate);
        let hold = if pattern.slide[i] {
            crate::next_hit_after(&pattern.hit, i, n)
                .map(|j| ((j + n - i) % n) as f32 + 0.5)
                .unwrap_or(length)
        } else {
            length
        };
        out.push(history::Note {
            step: i,
            note,
            cents: if nts { cents } else { 0.0 },
            hold,
            slide: pattern.slide[i],
        });
    }
    out
}

/// Phase-lock the overlay to the beat clock the scheduler stamped, not to a guessed block length.
fn overlay_clock<'a>(
    shared: &Shared,
    p: &MelodyParams,
    bpm: f32,
    now_ms: u64,
    loop_steps: f32,
    notes: &'a [history::Note],
) -> Option<history::Overlay<'a>> {
    if notes.is_empty() { return None; }
    let bpm64 = bpm as f64;
    if !(bpm64.is_finite() && bpm64 > 0.0) { return None; }
    let spb = p.steps_per_beat();
    Some(history::Overlay {
        notes,
        step_ms: (60_000.0 / (bpm64 * spb)) as f32,
        phase_steps: roll_phase_steps(shared, bpm, now_ms, spb) as f32,
        loop_steps,
        swing_steps: p.swing.value().clamp(0.0, 0.5),
    })
}

/// Push the right-to-left history, with the window in milliseconds derived from tempo and bar count.
fn push_history(
    ui: &AppWindow,
    shared: &Shared,
    p: &MelodyParams,
    pattern: &Pattern,
    pitch: &crate::PitchValues,
    bpm: f32,
    bars: f32,
    loop_steps: f32,
    now_ms: u64,
) {
    let window_ms = history_window_ms(bars, bpm, p.steps_per_beat(), p.loop_steps());
    let mut events = shared.snapshot();
    let notes = if p.history_overlay.value() {
        overlay_notes(p, pattern, pitch)
    } else {
        Vec::new()
    };
    let overlay = overlay_clock(shared, p, bpm, now_ms, loop_steps, &notes);
    let spb = p.steps_per_beat();
    let loop_steps = p.loop_steps();
    let unit = p.step_unit();
    let meter = crate::clock::meter_from_steps(p.cell_count().max(1) as u32, unit);
    let grid = history::GridSpec {
        step_ms: (60_000.0 / (bpm as f64 * spb)) as f32,
        steps_per_beat: spb as f32,
        // One bar is the generated pattern loop, expressed in quarter-note beats.
        beats_per_bar: (loop_steps as f64 / spb) as f32,
    };
    let roll = history::build_with_grid(
        &mut events,
        now_ms,
        window_ms,
        HISTORY_W,
        HISTORY_H,
        overlay.as_ref(),
        Some(&grid),
    );
    ui.set_history_bg(SharedString::from(roll.bg.as_str()));
    ui.set_history_bg_bars(SharedString::from(roll.bg_bars.as_str()));
    ui.set_history_grid(SharedString::from(roll.grid.as_str()));
    ui.set_history_grid_steps(SharedString::from(roll.grid_steps.as_str()));
    ui.set_history_grid_beats(SharedString::from(roll.grid_beats.as_str()));
    ui.set_history_grid_bars(SharedString::from(roll.grid_bars.as_str()));
    ui.set_history_notes(SharedString::from(roll.notes.as_str()));
    ui.set_history_notes_mid(SharedString::from(roll.notes_mid.as_str()));
    ui.set_history_notes_hard(SharedString::from(roll.notes_hard.as_str()));
    ui.set_history_pattern(SharedString::from(roll.pattern.as_str()));
    ui.set_history_pattern_slides(SharedString::from(roll.pattern_slides.as_str()));
    ui.set_history_lo(roll.lo);
    ui.set_history_hi(roll.hi);
    ui.set_history_ruler(ModelRc::new(VecModel::from(
        (roll.lo..roll.hi)
            .step_by(12)
            .map(|note| SharedString::from(note_label(note)))
            .collect::<Vec<_>>(),
    )));
    ui.set_history_range(SharedString::from(format!(
        "{} – {}",
        note_label(roll.lo),
        note_label(roll.hi - 1)
    )));
    ui.set_history_timing(SharedString::from(crate::clock::meter_label(
        meter,
        unit,
        loop_steps.max(1),
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
        LfoPolarity::UnipolarNegative => "-",
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
                // Depth is a share of the target range, so 1.0 is one full sweep.
                let depth = spec.depth.clamp(0.0, crate::modulation::LFO_MAX_DEPTH);
                match spec.polarity {
                    LfoPolarity::Bipolar => {
                        depth_up += depth;
                        depth_down += depth;
                    }
                    LfoPolarity::Unipolar => depth_up += depth,
                    LfoPolarity::UnipolarNegative => depth_down += depth,
                }
                let speed = match spec.rate_mode {
                    LfoRateMode::Hz if spec.freq_hz >= 10.0 => {
                        format!("{:.0}Hz", spec.freq_hz)
                    }
                    LfoRateMode::Hz if spec.freq_hz >= 1.0 => format!("{:.1}Hz", spec.freq_hz),
                    LfoRateMode::Hz => format!("{:.2}Hz", spec.freq_hz),
                    LfoRateMode::Beats => format!("{:.1}b", spec.rate_beats),
                };
                // The signal is already in target ranges, so × 100 is the signed percentage.
                let percent = signals[slot] * 100.0;
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
        let reduce_motion = stored.reduce_motion.unwrap_or(false);
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
        let edit_history = Arc::new(Mutex::new(EditHistory::default()));
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
            ui.set_reduce_motion(reduce_motion);
            let folder = setup_midi_dir.lock().map(|d| d.clone()).unwrap_or_default();
            ui.set_midi_folder(SharedString::from(folder.as_str()));
            {
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_apply_preset(move |index| apply_preset(&c, &p, index));
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
                ui.on_set_osc_wave(move |v| {
                    set(&c, &p.osc_wave, OscWave::from_index(v.clamp(0, OscWave::variants().len() as i32 - 1) as usize));
                });
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_set_markov_bias(move |v| set(&c, &p.markov_bias, (v.round() as i32).clamp(0, 255)));
            }
            {
                let (p, shared) = (setup_params.clone(), setup_shared.clone());
                let history = edit_history.clone();
                ui.on_edit_overlay(move |x, mode| {
                    if p.voice_mode.value() == VoiceMode::Polymeter { return; }
                    let hold = shared.history_hold_ms.load(Relaxed);
                    let now = if hold > 0 { hold } else { shared.epoch.elapsed().as_millis() as u64 };
                    let bpm = f32::from_bits(shared.tempo_bits.load(Relaxed));
                    let Some(step) = overlay_step_at(&p, &shared, bpm, now, x) else { return; };
                    if let Ok(mut history) = history.lock() {
                        history.record(p.edit_overlay.snapshot());
                    }
                    edit_step(&p, step, mode);
                    shared.edit_step.store(step as i32, Relaxed);
                });
                let (p, shared) = (setup_params.clone(), setup_shared.clone());
                let history = edit_history.clone();
                // phase: 0 = start, 1 = move, 2 = end. The start point fixes both the edited step
                // and its onset, so dragging one note never paints neighbouring steps.
                let gate_origin = Arc::new(Mutex::new(None::<(usize, f32)>));
                ui.on_edit_gate(move |x, snap, phase| {
                    if p.voice_mode.value() == VoiceMode::Polymeter { return; }
                    if phase == 2 {
                        if let Ok(mut origin) = gate_origin.lock() { *origin = None; }
                        return;
                    }
                    let hold = shared.history_hold_ms.load(Relaxed);
                    let now = if hold > 0 { hold } else { shared.epoch.elapsed().as_millis() as u64 };
                    let bpm = f32::from_bits(shared.tempo_bits.load(Relaxed));
                    let bpm = if bpm.is_finite() && bpm > 0.0 { bpm } else { 120.0 };
                    let spb = p.steps_per_beat();
                    let window_ms = history_window_ms(p.history_bars.value(), bpm, p.steps_per_beat(), p.loop_steps());
                    let step_x = (60_000.0 / (bpm as f64 * spb)) as f32 / window_ms as f32;
                    if phase == 0 {
                        let Some((step, fraction)) = overlay_position(&p, &shared, bpm, now, x) else {
                            return;
                        };
                        if let Ok(mut history) = history.lock() {
                            history.record(p.edit_overlay.snapshot());
                        }
                        if let Ok(mut origin) = gate_origin.lock() {
                            *origin = Some((step, x - fraction * step_x));
                        }
                        shared.edit_step.store(step as i32, Relaxed);
                        return;
                    }
                    let origin = gate_origin.lock().ok().and_then(|o| *o);
                    let Some((step, onset_x)) = origin else { return; };
                    let unit = snap.clamp(1.0 / 64.0, 1.0);
                    let raw = ((x - onset_x) / step_x).clamp(0.02, 1.0);
                    let length = ((raw / unit).round() * unit).clamp(0.02, 1.0);
                    p.edit_overlay.set_length(step, quantize_length(Some(length)));
                    shared.edit_step.store(step as i32, Relaxed);
                });
                let (p, shared) = (setup_params.clone(), setup_shared.clone());
                let history = edit_history.clone();
                ui.on_clear_edits(move || {
                    if let Ok(mut history) = history.lock() {
                        history.record(p.edit_overlay.snapshot());
                    }
                    p.edit_overlay.clear_all();
                    shared.edit_step.store(-1, Relaxed);
                });
                let (p, shared) = (setup_params.clone(), setup_shared.clone());
                let history = edit_history.clone();
                ui.on_undo_edit(move || {
                    let current = p.edit_overlay.snapshot();
                    if let Ok(mut history) = history.lock() {
                        if let Some(previous) = history.undo(current) {
                            p.edit_overlay.apply(&previous);
                        }
                    }
                    shared.edit_step.store(-1, Relaxed);
                });
                let (p, shared) = (setup_params.clone(), setup_shared.clone());
                let history = edit_history.clone();
                ui.on_redo_edit(move || {
                    let current = p.edit_overlay.snapshot();
                    if let Ok(mut history) = history.lock() {
                        if let Some(next) = history.redo(current) {
                            p.edit_overlay.apply(&next);
                        }
                    }
                    shared.edit_step.store(-1, Relaxed);
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
                    set(&c, lfo_depth_param(&p, slot), value.clamp(0.0, crate::modulation::LFO_MAX_DEPTH));
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
                {
                    // User-global, not project state: persist next to the theme and window scale.
                    let weak = ui.as_weak();
                    ui.on_toggle_reduce_motion(move || {
                        let Some(ui) = weak.upgrade() else { return };
                        let next = !ui.get_reduce_motion();
                        ui.set_reduce_motion(next);
                        let mut settings = globals::load();
                        settings.reduce_motion = Some(next);
                        globals::save(&settings);
                    });
                }
                let (p, c) = (setup_params.clone(), setup_ctx.clone());
                ui.on_toggle_history_overlay(move || {
                    set(&c, &p.history_overlay, !p.history_overlay.value())
                });
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
            ui.set_preset_names(ModelRc::new(VecModel::from(
                PRESETS.iter().map(|preset| SharedString::from(preset.name)).collect::<Vec<_>>(),
            )));
            ui.set_mod_rows(ModelRc::new(VecModel::from(Vec::<ModRow>::new())));
            ui.set_mod_status(SharedString::new());
            ui.set_mode_rows(ModelRc::new(VecModel::from(mode_rows())));
            ui.set_app_version(SharedString::from(crate::contact::VERSION));
            ui.set_settings_path(SharedString::from(crate::globals::config_path_display()));
            ui.set_credit_author(SharedString::from(crate::contact::AUTHOR));
            ui.set_credit_github(SharedString::from(crate::contact::GITHUB));
        };

        let cache = Arc::new(Mutex::new(Cache {
            gen_key: None,
            pattern: Pattern::default(),
            voice_preview: 0,
            view_key: None,
        }));
        let (update_params, update_shared, update_cache, update_context) =
            (self.params.clone(), self.shared.clone(), cache, context.clone());
        let update_scale_cell = self.scale.clone();
        let last_randomize_request = AtomicU32::new(update_shared.randomize_request.load(Relaxed));
        let last_regen_pulse = AtomicU32::new(update_shared.regen_pulse.load(Relaxed));
        // `u32::MAX` forces the first tick to publish whatever track name is already known.
        let last_track_rev = AtomicU32::new(u32::MAX);
        // History paths are expensive to rebuild; 30 fps is smooth for a scrolling piano roll.
        let last_history_ms = AtomicU64::new(0);
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
                ui.global::<FocusRing>().set_color(rgb(colors[3]));
                ui.set_theme_index(theme_index);
                globals::save(&global_settings(p, update_scale, &update_midi_dir));
            }
            let ceiling = p.step_ceiling() as f32;
            ui.set_max_steps(ceiling);
            // Show the value the generator will actually use, not one above a lowered ceiling.
            ui.set_steps(p.steps.value().min(ceiling));
            ui.set_round_steps(p.round_steps.value());
            ui.set_voices(p.voice_count() as i32);
            ui.set_voice_spread(p.voice_spread.value());
            ui.set_voice_mode(p.voice_mode.value().to_index() as i32);
            ui.set_voice_preview(p.voice_preview.value().clamp(0, MAX_VOICES as i32 - 1));
            let timing_locked = p.voice_mode.value() == VoiceMode::Polymeter;
            ui.set_timing_locked(timing_locked);
            if timing_locked { ui.set_edit_mode(0); }
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
            ui.set_scope_phase(scope_enabled(p, scope::PHASE));
            ui.set_scope_acid(scope_enabled(p, scope::ACID));
            ui.set_scope_feel(scope_enabled(p, scope::FEEL));
            ui.set_seed(p.seed.value());
            ui.set_phase(p.phase.value());
            ui.set_osc_rate(p.osc_rate.value());
            ui.set_osc_wave(p.osc_wave.value().to_index() as i32);
            ui.set_ca_rule(p.ca_rule.value() as f32);
            ui.set_symmetry(p.symmetry.value() as f32);
            ui.set_markov_bias(p.markov_bias.value() as f32);
            let loop_steps = p.loop_steps();
            let unit = p.step_unit();
            let meter = crate::clock::meter_from_steps(p.cell_count().max(1) as u32, unit);
            ui.set_steps_per_bar((loop_steps as f64 / p.steps_per_beat()) as f32);
            ui.set_timing_summary(SharedString::from(crate::clock::meter_label(
                meter,
                unit,
                loop_steps.max(1),
            )));
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
            ui.set_history_overlay_on(p.history_overlay.value());
            ui.set_slide(p.slide.value());
            ui.set_accent(p.accent.value());
            ui.set_octave(p.octave.value());

            let preview_clock = update_shared.pattern_clock();
            let gen_key = p.gen_params_at(preview_clock);
            let preview_pitch = p.pitch_values_at(preview_clock);
            let cur = update_shared.current_step.load(Relaxed);
            ui.set_current_step(cur);
            let edited_step = update_shared.edit_step.load(Relaxed);
            if edited_step >= 0 {
                ui.set_edit_status(SharedString::from(edit_status(&p, edited_step as usize)));
            } else {
                ui.set_edit_status(SharedString::new());
            }
            ui.set_history_running(update_shared.lfo_running.load(Relaxed));
            let mut c = update_cache.lock().unwrap_or_else(|poisoned| {
                log_line("WARNING: update cache mutex was poisoned; recovering its inner state");
                poisoned.into_inner()
            });
            let preview_voice = (p.voice_preview.value().max(0) as usize)
                .min(p.voice_count().saturating_sub(1));
            if c.gen_key != Some(gen_key) || c.voice_preview != preview_voice {
                let mut edit_view = crate::edits::EditView::default();
                p.edit_overlay.fill(&mut edit_view);
                let voice_key = p.voice_params(gen_key, preview_voice);
                c.pattern = generate_with(&voice_key, &edit_view);
                c.voice_preview = preview_voice;
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
            let view_key = (gen_key, cur, preview_pitch.root, gen_key.osc_wave, preview_voice);
            if c.view_key != Some(view_key) {
                let v = view::build(gen_key.kind, &c.pattern, c.pattern.steps, cur, preview_pitch.root, gen_key.osc_wave);
                push_view(&ui, &v);
                c.view_key = Some(view_key);
            }
            // Live clock: follow the newest note automatically. Entering an edit mode freezes the
            // scroll position at the edit start, so notes stay under the pointer while they are
            // being changed; leaving edit mode resumes live following on the next tick.
            let editing = ui.get_history_editing();
            let now = update_shared.epoch.elapsed().as_millis() as u64;
            let now_ms = if editing {
                let hold = update_shared.history_hold_ms.load(Relaxed);
                if hold > 0 {
                    hold
                } else {
                    update_shared.history_hold_ms.store(now, Relaxed);
                    now
                }
            } else {
                update_shared.history_hold_ms.store(0, Relaxed);
                now
            };
            let last = last_history_ms.load(Relaxed);
            if editing || now.saturating_sub(last) >= 33 {
                last_history_ms.store(now, Relaxed);
                push_history(&ui, &update_shared, p, &c.pattern, &preview_pitch, update_shared.sample_tempo(), history_bars_effective, gen_key.length, now_ms);
            }
        };
        let factory = move || {
            let ui = AppWindow::new()?;
            Ok(ui)
        };
        if self.params.logging.value() {
            log_line("editor spawn: opening parented Slint/baseview window");
        }
        let window = SlintWindow::open_parented(&parent, options, factory, setup, update, |_, _| {});
        if self.params.logging.value() {
            log_line("editor spawn: open_parented returned");
        }
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
    fn history_lengths_are_derived_pattern_loops() {
        // 16 steps at 1/16 is a 4/4 loop: 4 * 16 * 125 ms = 8 s at 120 BPM.
        assert_eq!(history_window_ms(4.0, 120.0, 4.0, 16), 8_000);
        assert_eq!(history_window_ms(4.0, 60.0, 4.0, 16), 16_000);
        assert_eq!(history_window_ms(1.0, 0.0, 4.0, 16), 2_000);
        // 13 steps at 1/16 is a 13/16 loop: 4 * 13 * 125 ms.
        assert_eq!(history_window_ms(4.0, 120.0, 4.0, 13), 6_500);
        // 16 steps at 1/8: 4 * 16 * 250 ms.
        assert_eq!(history_window_ms(4.0, 120.0, 2.0, 16), 16_000);
    }

    #[test]
    fn history_bar_length_is_the_derived_loop() {
        use crate::clock::{meter_from_steps, StepUnit};
        assert_eq!(meter_from_steps(16, StepUnit::Sixteenth), Some(crate::clock::Meter::new(4, 4)));
        assert_eq!(meter_from_steps(13, StepUnit::Sixteenth), Some(crate::clock::Meter::new(13, 16)));
        assert_eq!(meter_from_steps(16, StepUnit::Eighth), Some(crate::clock::Meter::new(16, 8)));
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
        let steps_center = Arc::new(Mutex::new(None::<(f32, f32)>));
        let steps_center_cb = steps_center.clone();
        ui.global::<ModRegistry>().on_register(move |target, visible, x, y, w, h| {
            if target >= 0 && visible {
                registered_in_cb.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if target == ModTarget::Steps.index() as i32 && visible {
                *steps_center_cb.lock().unwrap() = Some((x + w / 2.0, y + h / 2.0));
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
        let mut buffer = vec![slint::Rgb8Pixel::new(0, 0, 0); (WIDTH * HEIGHT) as usize];
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut dispatched = false;
        while Instant::now() < deadline {
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut buffer, WIDTH as usize);
            });
            if !dispatched {
                if let Some((x, y)) = *steps_center.lock().unwrap() {
                    let _ = window.try_dispatch_event(slint::platform::WindowEvent::PointerMoved {
                        position: slint::LogicalPosition::new(x, y),
                    });
                    dispatched = true;
                }
            }
            if dispatched
                && registered.load(std::sync::atomic::Ordering::Relaxed) > 0
                && ui.global::<ModHover>().get_target() == ModTarget::Steps.index() as i32
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(dispatched, "the Steps modulation target never registered");
        assert!(
            registered.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "modulation drop targets were never registered"
        );
        assert_eq!(
            ui.global::<ModHover>().get_target(),
            ModTarget::Steps.index() as i32,
            "hover timer did not report the parameter under the pointer"
        );

        // Keyboard path: a window shortcut fires with no control focused, and keeps firing after
        // Tab moves focus into one, whose key handler rejects it so it bubbles to the scope.
        let randomizes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = randomizes.clone();
        ui.on_randomize_pressed(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        window
            .try_dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "r".into() })
            .unwrap();
        window
            .try_dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Tab.into(),
            })
            .unwrap();
        window
            .try_dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "r".into() })
            .unwrap();
        assert_eq!(
            randomizes.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "the r shortcut did not survive Tab moving focus into a control"
        );

        // A closed drawer is parked off-window; it must also leave the tab order, or Tab walks
        // into its MIDI folder field and starts typing into a panel nobody can see.
        for _ in 0..150 {
            window
                .try_dispatch_event(slint::platform::WindowEvent::KeyPressed {
                    text: slint::platform::Key::Tab.into(),
                })
                .unwrap();
            assert!(
                !ui.get_text_focused(),
                "a closed Settings drawer must not take keyboard focus"
            );
        }
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
    fn target_views_point_the_depth_band_in_the_polarity_direction() {
        let params = |polarity| MelodyParams {
            lfo1_links: std::sync::atomic::AtomicU64::new(crate::modulation::target_bit(
                ModTarget::Steps,
            )),
            lfo1_depth: FloatParam::new(
                "LFO 1 Depth",
                1.0,
                FloatRange::Linear { min: 0.0, max: crate::modulation::LFO_MAX_DEPTH },
            ),
            lfo1_polarity: EnumParam::new("LFO 1 Polarity", polarity),
            ..MelodyParams::default()
        };
        let band = |polarity| {
            let views = target_views(&params(polarity), crate::modulation::LfoClock::default());
            let view = &views[ModTarget::Steps.index()];
            (view.depth_up, view.depth_down)
        };
        let (up, down) = band(LfoPolarity::Bipolar);
        assert!(up >= 1.0 && down >= 1.0, "bipolar {up}/{down}");
        let (up, down) = band(LfoPolarity::Unipolar);
        assert!(up >= 1.0 && down == 0.0, "unipolar {up}/{down}");
        let (up, down) = band(LfoPolarity::UnipolarNegative);
        assert!(up == 0.0 && down >= 1.0, "unipolar negative {up}/{down}");
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

    #[test]
    fn overlay_click_maps_to_the_display_step() {
        let p = MelodyParams::default();
        let shared = Shared::default();
        shared.last_beat.store(0.0f64.to_bits(), Relaxed);
        shared.beat_ms.store(1_000, Relaxed);
        // 4 bars at 120 BPM and 1/16 is 8000 ms; x = 0.875 is 1000 ms (8 steps) before now.
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 1_000, 1.0), Some(0));
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 1_000, 0.875), Some(8));
    }

    #[test]
    fn overlay_scrolls_before_the_first_processed_block() {
        let p = MelodyParams::default();
        let shared = Shared::default();
        assert_eq!(shared.beat_ms.load(Relaxed), 0);
        // 120 BPM, 1/16: 125 ms per step. With no scheduler anchor the wall clock supplies phase.
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 0, 1.0), Some(0));
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 125, 1.0), Some(1));
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 1_000, 1.0), Some(8));
        // x = 0.875 is 1000 ms behind the edge, so it lands back on the loop start.
        assert_eq!(overlay_step_at(&p, &shared, 120.0, 1_000, 0.875), Some(0));
    }

    #[test]
    fn built_in_presets_are_valid_for_the_current_bounds() {
        assert!(!PRESETS.is_empty());
        for preset in PRESETS {
            assert!(!preset.name.is_empty());
            assert!(preset.steps >= 1.0 && preset.steps <= MAX_STEPS as f32, "{}", preset.name);
            assert!(preset.pulses >= 0 && preset.pulses <= preset.steps as i32, "{}", preset.name);
            assert!((0.0..=1.0).contains(&preset.density), "{}", preset.name);
            assert!((0.0..=1.0).contains(&preset.probability), "{}", preset.name);
            assert!((0.0..=1.0).contains(&preset.seed), "{}", preset.name);
        }
    }

    /// Render one history window to PPM frames so the README GIF can be regenerated.
    /// `cargo test --lib write_ui_frames -- --ignored`, then `magick -delay 12 -loop 0 *.ppm out.gif`.
    #[test]
    #[ignore = "writes an animation frame sequence"]
    fn write_ui_frames() {
        use nice_plug::params::InternalParamMut;
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use slint::platform::{Platform, WindowAdapter};
        use std::io::Write as _;
        use std::rc::Rc;
        use std::time::{Duration, Instant};

        struct TestPlatform { window: Rc<MinimalSoftwareWindow>, start: Instant }
        impl Platform for TestPlatform {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(self.window.clone())
            }
            fn duration_since_start(&self) -> Duration { self.start.elapsed() }
        }

        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(TestPlatform { window: window.clone(), start: Instant::now() })).unwrap();
        window.set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
        let ui = AppWindow::new().unwrap();

        let p = MelodyParams::default();
        unsafe {
            p.mode._internal_set_plain_value(Mode::Euclidean);
            p.steps._internal_set_plain_value(16.0);
            p.pulses._internal_set_plain_value(5);
            p.density._internal_set_plain_value(0.7);
            p.velocity._internal_set_plain_value(0.85);
            p.gate._internal_set_plain_value(0.35);
        }
        let gen_key = p.gen_params();
        let pattern = crate::sequence::generate(&gen_key);

        ui.set_mode(p.mode.value().to_index() as i32);
        ui.set_steps(16.0);
        ui.set_pulses(5.0);
        ui.set_density(0.7);
        ui.set_probability(p.probability.value());
        ui.set_velocity(0.85);
        ui.set_gate(0.35);
        ui.set_seed(p.seed.value());
        ui.set_phase(p.phase.value());
        ui.set_osc_rate(p.osc_rate.value());
        ui.set_voices(1);
        ui.set_tuning_name("Minor".into());
        ui.set_root_pc(0);
        ui.set_octaves(2);
        ui.set_nts_on(false);
        ui.set_edo_active(false);
        ui.set_app_version(crate::contact::VERSION.into());
        let colors = p.theme.value().colors();
        let palette = ui.global::<Theme>();
        palette.set_c1(rgb(colors[0])); palette.set_c2(rgb(colors[1])); palette.set_c3(rgb(colors[2]));
        palette.set_hi(rgb(colors[3])); palette.set_bg1(rgb(colors[4])); palette.set_bg2(rgb(colors[5])); palette.set_bg3(rgb(colors[6]));
        palette.set_surface(rgb(colors[7])); palette.set_control(rgb(colors[8])); palette.set_control_hover(rgb(colors[9]));
        palette.set_text(rgb(colors[10])); palette.set_text_muted(rgb(colors[11])); palette.set_border(rgb(colors[12]));
        palette.set_cell_off(rgb(colors[13])); palette.set_on_accent(rgb(colors[14]));
        ui.global::<FocusRing>().set_color(rgb(colors[3]));
        ui.set_theme_index(p.theme.value().to_index() as i32);
        ui.set_history_bars(4.0);
        ui.set_history_overlay_on(false);
        ui.set_steps_per_bar(16.0);
        ui.set_mode_math(crate::help::math(&gen_key).into());
        ui.show().unwrap();

        let mut events = Vec::new();
        let mut t = 0u64;
        for i in 0..40u64 {
            let note = 48 + [0u8, 4, 7, 12, 7, 4][(i % 6) as usize];
            events.push(crate::history::Ev { t_ms: t, note, vel: 40 + (i % 5) as u8 * 20, voice: 0, on: true, cents: 0.0 });
            events.push(crate::history::Ev { t_ms: t + 190, note, vel: 0, voice: 0, on: false, cents: 0.0 });
            t += 260;
        }
        let window_ms = 4000u64;
        let dir = std::env::temp_dir().join("nelody-seq-frames");
        std::fs::create_dir_all(&dir).unwrap();
        let mut buffer = vec![slint::Rgb8Pixel::new(0, 0, 0); (WIDTH * HEIGHT) as usize];
        // One 6-note phrase (1560 ms) over 12 frames keeps the loop seamless.
        let frames = 12u64;
        for frame in 0..frames {
            let now = 260 * 6 + frame * 130;
            let mut ev = events.clone();
            let roll = crate::history::build_with_grid(&mut ev, now, window_ms, 800.0, 200.0, None, None);
            ui.set_history_bg(roll.bg.as_str().into());
            ui.set_history_bg_bars(roll.bg_bars.as_str().into());
            ui.set_history_grid(roll.grid.as_str().into());
            ui.set_history_grid_steps(roll.grid_steps.as_str().into());
            ui.set_history_grid_beats(roll.grid_beats.as_str().into());
            ui.set_history_grid_bars(roll.grid_bars.as_str().into());
            ui.set_history_notes(roll.notes.as_str().into());
            ui.set_history_notes_mid(roll.notes_mid.as_str().into());
            ui.set_history_notes_hard(roll.notes_hard.as_str().into());
            ui.set_history_lo(roll.lo);
            ui.set_history_hi(roll.hi);
            ui.set_history_ruler(ModelRc::new(VecModel::from(
                (roll.lo..roll.hi)
                    .step_by(12)
                    .map(|note| SharedString::from(note_label(note)))
                    .collect::<Vec<_>>(),
            )));
            ui.set_history_range(
                format!("{} – {}", note_label(roll.lo), note_label(roll.hi - 1)).into(),
            );
            ui.set_history_timing("4/4".into());
            let cur = (frame % 16) as i32;
            let view = view::build(gen_key.kind, &pattern, pattern.steps, cur, p.root.value(), gen_key.osc_wave);
            push_view(&ui, &view);
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| { renderer.render(&mut buffer, WIDTH as usize); });
            let path = dir.join(format!("frame-{frame:03}.ppm"));
            let mut out = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            write!(out, "P6\n{WIDTH} {HEIGHT}\n255\n").unwrap();
            for px in &buffer { out.write_all(&[px.r, px.g, px.b]).unwrap(); }
        }
        println!("wrote {frames} frames to {}", dir.display());
    }

    /// `ui/shared.slint` is duplicated verbatim in nrum seq and nelody seq. Editing it must be a
    /// deliberate act: change it, copy it to the sibling repository, then update both hashes.
    #[test]
    fn the_shared_ui_matches_its_pinned_hash() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/shared.slint"))
            .expect("ui/shared.slint is missing");
        let mut hash = 0xcbf29ce484222325u64;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        assert_eq!(
            hash, 0xeba7569c8fb94ad5,
            "ui/shared.slint changed: copy it to the sibling repository and update both pins"
        );
    }
}
