//! Per-step manual edits: hit overrides, accent, probability, ratchets and microtiming.
//!
//! The audio thread must never take a lock, so the persistent `EditStore` mirrors its data in
//! atomics. `EditView` is the plain, copyable snapshot the generators and scheduler read. A
//! revision counter makes a manual edit look like a parameter change to the snapshot comparison.

use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering::Relaxed};
use std::sync::Arc;

use nice_plug::params::persist::PersistentField;
use serde::{Deserialize, Serialize};

use crate::sequence::MAX_STEPS;

/// Hit override: 0 = follow the generator, 1 = force on, 2 = force off.
pub const HIT_AUTO: u8 = 0;
pub const HIT_ON: u8 = 1;
pub const HIT_OFF: u8 = 2;
/// Probability override: 255 = follow the global probability.
pub const PROB_AUTO: u8 = 255;
/// Accent override: 0 = follow the generated accent.
pub const ACCENT_AUTO: u8 = 0;
/// Ratchet count; 1 means no ratchet.
pub const RATCHET_AUTO: u8 = 1;
/// Per-step length override; 0 means follow the global gate.
pub const LENGTH_AUTO: u8 = 0;

/// Convert a 0.02..1.0 step-length fraction into the stored byte, or `LENGTH_AUTO` for `None`.
pub fn quantize_length(value: Option<f32>) -> u8 {
    match value {
        None => LENGTH_AUTO,
        Some(v) => ((v.clamp(0.02, 1.0) * 255.0).round() as u8).max(1),
    }
}

/// Decode one stored length byte. `None` means the global gate still applies.
pub fn length_fraction(value: u8) -> Option<f32> {
    (value != LENGTH_AUTO).then(|| (value as f32 / 255.0).clamp(0.02, 1.0))
}

/// Plain snapshot consumed by generation, the scheduler, export and preview.
#[derive(Clone, Debug, PartialEq)]
pub struct EditView {
    pub hit: [u8; MAX_STEPS],
    pub accent: [u8; MAX_STEPS],
    pub probability: [u8; MAX_STEPS],
    pub ratchet: [u8; MAX_STEPS],
    /// Microtiming as a fraction of one step, -127..127 (0 = on the grid).
    pub offset: [i8; MAX_STEPS],
    /// Per-step gate/length override; 0 follows the global gate.
    pub length: [u8; MAX_STEPS],
}

impl Default for EditView {
    fn default() -> Self {
        Self {
            hit: [HIT_AUTO; MAX_STEPS],
            accent: [ACCENT_AUTO; MAX_STEPS],
            probability: [PROB_AUTO; MAX_STEPS],
            ratchet: [RATCHET_AUTO; MAX_STEPS],
            offset: [0; MAX_STEPS],
            length: [LENGTH_AUTO; MAX_STEPS],
        }
    }
}

impl EditView {
    pub fn hit_override(&self, step: usize) -> u8 {
        self.hit[step.min(MAX_STEPS - 1)]
    }

    /// `None` means the global probability still applies.
    pub fn probability_at(&self, step: usize) -> Option<f32> {
        let value = self.probability[step.min(MAX_STEPS - 1)];
        (value != PROB_AUTO).then(|| value as f32 / 254.0)
    }

    /// `None` means the generated accent still applies.
    pub fn accent_at(&self, step: usize) -> Option<f32> {
        let value = self.accent[step.min(MAX_STEPS - 1)];
        (value != ACCENT_AUTO).then(|| (value as f32 / 255.0).clamp(0.05, 1.0))
    }

    pub fn ratchet_at(&self, step: usize) -> u8 {
        self.ratchet[step.min(MAX_STEPS - 1)].clamp(1, 4)
    }

    pub fn offset_at(&self, step: usize) -> f32 {
        self.offset[step.min(MAX_STEPS - 1)] as f32 / 127.0
    }

    /// `None` means the global gate still applies.
    pub fn length_at(&self, step: usize) -> Option<f32> {
        length_fraction(self.length[step.min(MAX_STEPS - 1)])
    }
}

/// One edited step in the project state. Only non-default cells are serialized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditCell {
    pub step: u16,
    #[serde(default)]
    pub hit: u8,
    #[serde(default)]
    pub accent: u8,
    #[serde(default = "auto_probability")]
    pub probability: u8,
    #[serde(default = "auto_ratchet")]
    pub ratchet: u8,
    #[serde(default)]
    pub offset: i8,
    /// Per-step gate/length; 0 follows the global gate.
    #[serde(default)]
    pub length: u8,
}

fn auto_probability() -> u8 { PROB_AUTO }
fn auto_ratchet() -> u8 { RATCHET_AUTO }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EditState {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub cells: Vec<EditCell>,
}

/// Lock-free mirror of the edit state. Reads and writes are single-byte atomics; the revision is
/// bumped after a change so the audio thread knows to refresh its plain `EditView`.
pub struct EditStore {
    hit: [AtomicU8; MAX_STEPS],
    accent: [AtomicU8; MAX_STEPS],
    probability: [AtomicU8; MAX_STEPS],
    ratchet: [AtomicU8; MAX_STEPS],
    offset: [AtomicU8; MAX_STEPS],
    length: [AtomicU8; MAX_STEPS],
    revision: AtomicU32,
}

impl Default for EditStore {
    fn default() -> Self {
        Self {
            hit: std::array::from_fn(|_| AtomicU8::new(HIT_AUTO)),
            accent: std::array::from_fn(|_| AtomicU8::new(ACCENT_AUTO)),
            probability: std::array::from_fn(|_| AtomicU8::new(PROB_AUTO)),
            ratchet: std::array::from_fn(|_| AtomicU8::new(RATCHET_AUTO)),
            offset: std::array::from_fn(|_| AtomicU8::new(0)),
            length: std::array::from_fn(|_| AtomicU8::new(LENGTH_AUTO)),
            revision: AtomicU32::new(0),
        }
    }
}

impl fmt::Debug for EditStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditStore").field("revision", &self.revision()).finish()
    }
}

impl EditStore {
    pub fn revision(&self) -> u32 { self.revision.load(Relaxed) }

    fn bump(&self) { self.revision.fetch_add(1, Relaxed); }

    pub fn hit(&self, step: usize) -> u8 {
        self.hit[step.min(MAX_STEPS - 1)].load(Relaxed)
    }

    pub fn set_hit(&self, step: usize, value: u8) {
        self.hit[step.min(MAX_STEPS - 1)].store(value.min(HIT_OFF), Relaxed);
        self.bump();
    }

    pub fn accent(&self, step: usize) -> u8 {
        self.accent[step.min(MAX_STEPS - 1)].load(Relaxed)
    }

    pub fn set_accent(&self, step: usize, value: u8) {
        self.accent[step.min(MAX_STEPS - 1)].store(value, Relaxed);
        self.bump();
    }

    pub fn probability(&self, step: usize) -> u8 {
        self.probability[step.min(MAX_STEPS - 1)].load(Relaxed)
    }

    pub fn set_probability(&self, step: usize, value: u8) {
        self.probability[step.min(MAX_STEPS - 1)].store(value, Relaxed);
        self.bump();
    }

    pub fn ratchet(&self, step: usize) -> u8 {
        self.ratchet[step.min(MAX_STEPS - 1)].load(Relaxed).clamp(1, 4)
    }

    pub fn set_ratchet(&self, step: usize, value: u8) {
        self.ratchet[step.min(MAX_STEPS - 1)].store(value.clamp(1, 4), Relaxed);
        self.bump();
    }

    pub fn offset(&self, step: usize) -> i8 {
        self.offset[step.min(MAX_STEPS - 1)].load(Relaxed) as i8
    }

    pub fn set_offset(&self, step: usize, value: i8) {
        self.offset[step.min(MAX_STEPS - 1)].store(value as u8, Relaxed);
        self.bump();
    }

    pub fn length(&self, step: usize) -> u8 {
        self.length[step.min(MAX_STEPS - 1)].load(Relaxed)
    }

    pub fn set_length(&self, step: usize, value: u8) {
        self.length[step.min(MAX_STEPS - 1)].store(value, Relaxed);
        self.bump();
    }

    /// `None` means the global gate still applies.
    pub fn length_at(&self, step: usize) -> Option<f32> {
        length_fraction(self.length(step))
    }

    pub fn clear_cell(&self, step: usize) {
        self.set_hit(step, HIT_AUTO);
        self.set_accent(step, ACCENT_AUTO);
        self.set_probability(step, PROB_AUTO);
        self.set_ratchet(step, RATCHET_AUTO);
        self.set_offset(step, 0);
        self.set_length(step, LENGTH_AUTO);
    }

    pub fn clear_all(&self) {
        for step in 0..MAX_STEPS {
            self.hit[step].store(HIT_AUTO, Relaxed);
            self.accent[step].store(ACCENT_AUTO, Relaxed);
            self.probability[step].store(PROB_AUTO, Relaxed);
            self.ratchet[step].store(RATCHET_AUTO, Relaxed);
            self.offset[step].store(0, Relaxed);
        }
        self.bump();
    }

    /// Fill the audio thread's plain snapshot. Called only when the revision changes.
    pub fn fill(&self, out: &mut EditView) {
        for step in 0..MAX_STEPS {
            out.hit[step] = self.hit[step].load(Relaxed);
            out.accent[step] = self.accent[step].load(Relaxed);
            out.probability[step] = self.probability[step].load(Relaxed);
            out.ratchet[step] = self.ratchet[step].load(Relaxed);
            out.offset[step] = self.offset[step].load(Relaxed) as i8;
            out.length[step] = self.length[step].load(Relaxed);
        }
    }

    pub fn snapshot(&self) -> EditState {
        let mut cells = Vec::new();
        for step in 0..MAX_STEPS {
            let hit = self.hit[step].load(Relaxed);
            let accent = self.accent[step].load(Relaxed);
            let probability = self.probability[step].load(Relaxed);
            let ratchet = self.ratchet[step].load(Relaxed);
            let offset = self.offset[step].load(Relaxed) as i8;
            let length = self.length[step].load(Relaxed);
            if hit != HIT_AUTO
                || accent != ACCENT_AUTO
                || probability != PROB_AUTO
                || ratchet != RATCHET_AUTO
                || offset != 0
                || length != LENGTH_AUTO
            {
                cells.push(EditCell { step: step as u16, hit, accent, probability, ratchet, offset, length });
            }
        }
        EditState { version: 1, cells }
    }

    pub fn apply(&self, state: &EditState) {
        self.clear_all();
        for cell in &state.cells {
            let step = cell.step as usize;
            if step >= MAX_STEPS { continue; }
            self.hit[step].store(cell.hit.min(HIT_OFF), Relaxed);
            self.accent[step].store(cell.accent, Relaxed);
            self.probability[step].store(cell.probability, Relaxed);
            self.ratchet[step].store(cell.ratchet.clamp(1, 4), Relaxed);
            self.offset[step].store(cell.offset as u8, Relaxed);
            self.length[step].store(cell.length, Relaxed);
        }
        self.bump();
    }
}

impl<'a> PersistentField<'a, EditState> for Arc<EditStore> {
    fn set(&self, new_value: EditState) { self.apply(&new_value); }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&EditState) -> R,
    {
        f(&self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_leaves_every_cell_to_the_generator() {
        let view = EditView::default();
        for step in 0..MAX_STEPS {
            assert_eq!(view.hit_override(step), HIT_AUTO);
            assert_eq!(view.probability_at(step), None);
            assert_eq!(view.accent_at(step), None);
            assert_eq!(view.ratchet_at(step), 1);
            assert_eq!(view.offset_at(step), 0.0);
            assert_eq!(view.length_at(step), None);
        }
    }

    #[test]
    fn edits_survive_a_snapshot_round_trip() {
        let store = EditStore::default();
        store.set_hit(7, HIT_OFF);
        store.set_probability(7, 127);
        store.set_accent(1015, 200);
        store.set_ratchet(0, 4);
        store.set_offset(3, -64);
        store.set_length(9, quantize_length(Some(0.42)));
        let state = store.snapshot();
        let restored = EditStore::default();
        restored.apply(&state);
        let mut original = EditView::default();
        let mut copy = EditView::default();
        store.fill(&mut original);
        restored.fill(&mut copy);
        assert_eq!(original, copy);
    }

    #[test]
    fn clearing_a_cell_restores_the_generator_defaults() {
        let store = EditStore::default();
        store.set_hit(1, HIT_ON);
        store.set_probability(1, 10);
        store.set_length(1, quantize_length(Some(0.5)));
        store.clear_cell(1);
        let mut view = EditView::default();
        store.fill(&mut view);
        assert_eq!(view.hit_override(1), HIT_AUTO);
        assert_eq!(view.probability_at(1), None);
        assert_eq!(view.length_at(1), None);
    }

    #[test]
    fn length_overrides_round_trip_and_clamp() {
        assert_eq!(length_fraction(LENGTH_AUTO), None);
        assert!((length_fraction(quantize_length(Some(0.0))).unwrap() - 0.02).abs() < 1e-6);
        assert!((length_fraction(quantize_length(Some(9.0))).unwrap() - 1.0).abs() < 1e-6);
        let store = EditStore::default();
        store.set_length(0, quantize_length(Some(0.25)));
        assert!((store.length_at(0).unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn every_edit_bumps_the_revision() {
        let store = EditStore::default();
        let start = store.revision();
        store.set_hit(0, HIT_ON);
        store.set_accent(0, 1);
        store.set_probability(0, 1);
        store.set_ratchet(0, 2);
        store.set_offset(0, 1);
        store.set_length(0, 32);
        store.clear_cell(0);
        assert!(store.revision() > start + 6);
    }
}
