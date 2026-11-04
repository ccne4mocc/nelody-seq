# `_Nelody` / nelody seq — Sequencer Mode Parameter & Calculation Overview

Source read: `src/modes.rs`, `src/lib.rs`, `src/sequence/mod.rs`, all `src/sequence/*.rs`, `src/modulation.rs`, `src/tuning.rs`.
This is a working reference for changing mode calculations. It separates **what the host parameter means**, **how it becomes `GenParams`**, **how the generated line is transformed**, and **how it becomes pitch/MIDI**.

- `n = steps` = number of integer grid cells the generator fills.
- `d = density`, `φ = 1.618033988749894`, `φ⁻¹ = φ − 1`.
- `pulse_ratio = min(pulses,n) / n`.
- ACID weight bank: `ACID_W = [1.0, 0.55, 0.45, 0.35, 0.3, 0.25, 0.2]`.
- `pitch_delay(n) = min(3, n−1)`.

---

## 1. Shared parameter transfer: host → GenParams → Pattern → pitch/MIDI

| Host parameter | `GenParams` field | Transfer / calculation | Consumed by |
|---|---|---|---|
| `mode` | `kind` | Selected enum variant; also drives `Mode::min_steps()` and `Mode::steps_for_mode()`. | Dispatch in `run_mode()` |
| `steps` (1..1024) | `steps`, `length` | `raw = clamp(steps, 1, ceiling)`; `cells = round(raw)` if `round_steps` else `ceil(raw)`, clamped 1..ceiling. `GenParams.steps = cells`; `GenParams.length = cells` when snapped, otherwise `raw`. | Generators read `steps`; scheduler reads `length` |
| `max_steps` (64..1024) | ceiling | `step_ceiling() = clamp(max_steps, 64, 1024)`. UI-side `steps_for_mode()` may raise `steps` and `max_steps` to the mode floor; the generator never sees `max_steps`. | `cell_count_for()`; LFO `MaxSteps` |
| `round_steps` | — | Chooses `round` vs `ceil` for `cells`. | `gen_params()` |
| `pulses` (0..1024) | `pulses` | `q.pulses = min(pulses, steps)`, except **Cellular** uses `min(pulses, CA_ROWS−1) = 15`. Meaning is mode-specific. `pulses == 0` hard-mutes every mode except Cellular and Life. | Generators |
| `rotate` (0..1023) | `rotate` | `q.rotate %= steps`. Actual shift is `(rotate + variation) % steps`. | Post-transform |
| `density` (0.01..1) | `density` | Passed through. Often a model coefficient or probability, not a hit-count density. `density <= 0` clears every hit after generation. | Generators; global mute |
| `probability` (0..1) | `probability` | Applied **once after** generation: each existing hit is retained with `rng.unit() < probability`. Nelody generators do not read `probability` internally. | `generate_impl()` |
| `velocity` (.01..1) | `velocity` | Not read by generators. At emission: `schedule_velocity × pattern.acc`. | Scheduler |
| `seed` (0..1) | `seed` | Normalized deterministic seed. Used directly as phase/start in many modes, via RNG in others, and ignored by some generators while the shared accent RNG still reads it. | Generators/accents |
| `phase` (0..1) | `phase` | Mode-specific: map start, word offset, direction, mutation, approach side, curve rotation. | Generators |
| `osc_rate` (.125..8) | `osc_rate` | Oscillator cycles, Markov leapiness. | Oscillator, Markov |
| `osc_wave` (8 enum values) | `osc_wave` | `% 8`; oscillator/LFO waveform. | Oscillator, LFO signal |
| `ca_rule` (0..255) | `ca_rule` | Cellular Wolfram rule, Sieve moduli, Life rule family. | Cellular/Sieve/Life |
| `symmetry` (1..8) | `symmetry` | Cellular seed policy, L-system family, Life seed shape. | Cellular/L-system/Life |
| `markov_bias` (0..255) | `markov_bias` | Four 2-bit fields: rest, repeat, role, novelty. | Markov only |
| `variation` (persisted `u32`) | `variation` | Incremented by Generate either direction and by regenerate-after-N. Always contributes `variation % steps` to rotation and seeds most generators/RNGs. | Rotation + generators |
| `accent` (0..1) | `accent` | Acid flag probability; not a velocity. In acid modes, chance a note's `Pattern.acc` is `.95` (otherwise `.76`). | Acid modes, Arpeggiator/Markov flags |
| `slide` (0..1) | `slide` | Acid slide probability; also influences note transition/Legato behavior. | Acid modes + scheduler |
| `oct_jump` (0..1) | `octave` | Acid octave-jump probability. | Acid modes |
| `invert` | `invert` | After rotation: fills resting contour for non-absolute lines, then flips every hit/rest. | Post-transform |
| `mirror` | `mirror` | After invert: reverses every per-step array; sets `Pattern.mirrored`. | Post-transform |
| `edit_overlay` | `edit_rev` | Revision is part of the cache key. `EditView` controls hit/accent/probability in display space; length/ratchet/offset are scheduler-only. | Generation + scheduler |
| `rate` | — | `steps_per_beat`: 1/4=1, 1/8=2, 1/16=4, 1/32=8, 1/64=16; dotted `/1.5`; triplet ×1.5. `step_len = 1 / steps_per_beat` beats. | Scheduler |
| `gate` (.02..1) | — | Fraction of a step. Per-step edit length overrides; ratchet gate capped to substep spacing. | Scheduler |
| `swing` (0..0.5) | — | Only odd steps are delayed, by `swing × step_len`. | Scheduler |
| `regenerate_every` (0..256) | — | Counted in complete loop lengths, not grid cells. `automatic_interval` makes base `0` a hard off: an LFO cannot switch it on. | Scheduler |
| `randomize_after_lengths` (0..256) | — | Same complete-loop counting; requests editor-side randomize. Base `0` is hard off. | Scheduler |
| `root` (0..11) | — | Pitch class of tonic, 0=C. Added after scale degree. | `pitch_for_values()` |
| `scale` | — | `Tuning` table or `Edo`; selects degree→cents mapping. | `pitch_for_values()` |
| `edo` (1..72) | — | Equal divisions per octave, only used by `Tuning::Edo`; `MAX_EDO = 72`. | `pitch_for_values()` |
| `root_cents` (−50..50) | — | Fine tonic detune, added before semitone rounding. | `pitch_for_values()` |
| `midi_root` (0..120) | — | MIDI note anchor; base = `midi_root + root + semis`. | `pitch_for_values()` |
| `octaves` (1..6) | — | Degree span for the default `pitch(u, octaves, edo, root_cents)` path. | `pitch_for_values()` |
| `nts` (bool) | — | Enables microtonal pitch bend; bend resets to centre when voice stops. | Scheduler |
| `bend_range` (1..24) | — | Pitch-bend range in semitones; RPN 0 is sent so the synth matches. | Scheduler |
| `history_bars`, `history_overlay` | — | Piano-roll view/history only; not used by generation or pitch. | Editor/history |

### Generation pipeline

1. `gen_params()` builds the plain snapshot from host parameters.
2. `gen_params_at(clock)` overlays linked LFO values: `MaxSteps` first, then `Steps` quantized to cells, then `Pulses`/`Rotate` clamped. It also modulates density, probability, velocity, seed, phase, osc rate, CA rule, accent, slide and octave.
3. `generate_with()` clamps `steps`, `pulses`, `rotate`, `symmetry`, `osc_wave`; runs `run_mode()`; sets `out.acid |= kind.is_acid()`.
4. Probability is applied per step once. Dropping a hit also clears its slide flag.
5. Hard mute: `density <= 0` or (`kind` is not Cellular/Life and `pulses == 0`) clears hits and slides.
6. Rotation `shift = (rotate % steps + variation % steps) % steps` reorders every per-step array.
7. Invert, mirror, metric accents, then manual edits.
8. Scheduler maps `step_index(k, length)` to a `Pattern` cell, applies swing/microtiming/ratchets/gate, and computes pitch through `pitch_for_values()`.
9. MIDI: one voice, channel 0 (MIDI channel 1). Pitch bend only while `nts`; slides hold until the next onset plus `SLIDE_OVERLAP = 0.5` step.

---

## 2. Per-mode parameters, calculations and transfers

“Pitch path” shows which `pitch_for_values()` branch the mode publishes: `absolute` (`semis + 12×oct`), `slotted` (`pitch_slot(slots, oct, …)`), `acid` (`pitch_in_octave(u, oct, …)`), or default (`pitch(u, octaves, …)`). All rows also pass through `generate_impl()` at `_Nelody/src/sequence/mod.rs:470`; the `Code reference` column lists the mode-specific implementation.

| # | Mode (group) | Controls actually read | Calculation / mapping | Pitch path / output | Code reference |
|---:|---|---|---|---|---|
| 0 | Euclidean (Rhythm) | `pulses`, `density`, `seed`, `phase`, `variation` | `k = requested_hits`: `round(min(pulses,n)×d)`, forced to `1..n` when `pulses>0 && d>0`. Hit `euclid_on(i,k,n)` = `(i×k) mod n < k`. `phase0 = frac(seed + phase + variation×φ⁻¹)`; onset index `j` gets `u = frac(phase0 + j×φ⁻¹)`. | Default `pitch(u, octaves, …)`. Accent `1.0` first onset, else `.78`. | _Nelody/src/sequence/euclidean.rs:13 generate; :3 requested_hits; _Nelody/src/sequence/mod.rs:270 euclid_on |
| 1 | Golomb Ruler (Number Theory) | `pulses`, `density` (gate), `seed`, `phase`, `variation` | `order = clamp(pulses,2,12)`; `effective_order` drops order while ruler length+1 > n. `variation%2` mirrors the ruler. `place()` tries stretch from widest down in 64 steps; keeps the first rounding whose pairwise mark differences are all distinct. Circular gaps determine `ratio = gap/max_gap`. `phase0 = frac(seed + phase + variation×φ⁻¹)`; each mark gets `u = frac(phase0 + ratio×.9)`, `orbit = ratio`, accent `.65+.35ratio`. | Default pitch. `n<2` forces one hit at step 0. | _Nelody/src/sequence/golomb.rs:72 generate; :18 ruler_order; :24 effective_order; :50 place |
| 2 | Logistic Map (Chaos / Physics) | `pulses`, `density`, `seed`, `phase`, `variation` | `wanted = round(min(pulses,n)×d).max(1)`. `r = 3.5 + .5d`; `x0 = clamp(.05+.9×frac(seed+phase+variation×φ⁻¹), .001,.999)`. 16 burn-in iterations, resetting to `.37` on collapse; orbit and `orbit_next` stored. `top_k(orbit, n, wanted)` selects hits. `u = normalize01(orbit[(i+delay)%n], observed lo/hi)`, `delay = min(3,n−1)`. | Default pitch. Accent `.6+.4u`; `map_param = r`. | _Nelody/src/sequence/logistic.rs:18 generate; :3 r_of; :8 x0; _Nelody/src/sequence/mod.rs:377 pitch_delay; :382 normalize01; :360 top_k |
| 3 | Eulerian Walk (Geometric) | `pulses`, `density` (gate), `seed`, `variation` | `nv = clamp(pulses,3,8)` vertices. Seeded shuffle builds a directed cycle over all vertices plus 1–2 extra short cycles; each vertex gets 4 adjacency slots; Hierholzer circuit. Step reads `circuit[i % ne]`; `state = destination`; hit if `dest != 0`. `u = (dest−1)/(nv−2).max(1)`; `slots = (dest−1)%7` or `i%7` on rests; accent `.95` for tonic, `.76` otherwise. | Slotted pitch. Fallback forces step 0 if the circuit is all rests. Density only gates. | _Nelody/src/sequence/eulerian.rs:5 generate; _Nelody/src/sequence/mod.rs:275 shuffle |
| 4 | Arnold Circle Map (Chaos / Physics) | `pulses`, `density`, `seed`, `phase`, `variation` | `Ω = max(1,min(pulses,n))/n`; `K = (1−d)×1.4` (high density = weak coupling). `θ = frac(seed+phase+variation×φ⁻¹)`. Iterate `θ' = θ + Ω − K/(2π)sin(2πθ)`; fire when `floor(next+1e−9) >= 1`; wrap `θ`. If no wrap, force the highest-θ step. | Default pitch using `θ` as `u`. `theta`/`orbit`/`u` = θ; accent `.65+.35θ`; `map_param = K`. | _Nelody/src/sequence/circle_map.rs:9 generate; :4 coupling |
| 5 | Mozart Dice (Stochastic) | `pulses` (dice/bar), `density` (transposition), `seed`/`variation` | `dice = clamp(pulses,1,3)`. Each 4-step bar rolls `dice` d6 and sums. Table index: 1 die `(sum−1)×2` clamped 10; 2 dice `sum−2`; 3 dice `(sum−3)×10/15`. 11-entry bank of `(velocity level 0..3, contour 0..1)`. `shift = (rng.unit()×.4−.2)×d` transposes contour. | Default pitch. `acc = VEL_LEVEL[level]` (`0,.45,.72,1`); `roll` = dice sum. Fallback hit 0 if the bank cell is all rests. | _Nelody/src/sequence/dice.rs:20 generate |
| 6 | Tonnetz Walk (Music Theory) | `pulses` (walk length), `density` (onset gate), `seed`/`variation` | `l = clamp(pulses,1,8)`; `dens = .4+.6d`. Start triangle `(1,2,up)`. Every `l` steps pick a P/L/R transform (not the same twice) within the 5×5 lattice; triad root `= (7a+4b+(up?0:4)) mod 12`; parsimonious voice leading keeps common tones; octave-shift voices into a playable window. Arpeggiate sorted voices: `j=i%l`, `semi = voices[j%3] + 12×((j/3)%2)`. Hit on `j==0` or `rng<dens`. | Absolute pitch (`semis`), `tri=(a,b,up)`, `u=(semi+12)/36`; accent `.95` on first arpeggio note else `.72`. | _Nelody/src/sequence/tonnetz.rs:64 generate; :6 tonnetz_transform; :19 tonnetz_root_pc; :30 lead |
| 7 | Cellular Automaton (Cellular) | `ca_rule`, `symmetry` (seed), `pulses`/`phase` (row), `density` (gate), `seed`/`variation` | `rule = ca_rule==0 ? 30 : ca_rule`. Seed: `symmetry=1` centre cell; `2` alternating stripes; `3` all live except cell 0; else `variation==0` centre, otherwise random bits. 16 rows on a ring. `selected_row = 0` if `n<=1`, else `(pulses + round(phase×16)) % 16`. Gate `= .35+.65d`; hit if selected cell live and (`d>=.99` or local RNG `< gate`). | Slotted pitch (via `column_code` below). `u = last-four-generations 4-bit column code / 15`; accent `.82`; `ca_play` = selected row. | _Nelody/src/sequence/cellular.rs:25 generate; :16 selected_row; :12 column_code; :3 nbr |
| 8 | Markov Walk (Stochastic) | `pulses` (activity), `density` (rest/event), `osc_rate` (leapiness), `markov_bias`, `seed`/`variation` | `event_chance = clamp(d × (.25 + pulse_ratio×.75) × (1−rest×.12), .05,1)`, `rest = bias&3`. Sounding if `n==1`, `(i==0 && d>=.5)`, or `rng < event_chance`. `prox_exp = 2.8×(1−(osc_rate−.125)/7.875)`. Weight for degree `b`: `ACID_W[b] × (1/(1+abs(b−deg)))^prox_exp`; chord tones 0/2/4 ×(`1+role×.35`); leaps `abs(b−deg)>=3` ×(`1+novelty×.35`); self degree ×(`1+repeat×.6`). | Acid pitch path (`pitch_in_octave`). `u=(deg+.5)/7`, `state=deg+1`, accent `.95` root else `.76`; `out.acid = true`. | _Nelody/src/sequence/markov.rs:10 generate; :4 proximity_exp; _Nelody/src/sequence/mod.rs:302 ACID_W; :284 pick |
| 9 | Oscillator / Phase (Rhythm) | `pulses`, `density`, `osc_rate`, `osc_wave`, `phase`, `seed`, `variation` | `wanted = requested_hits`. `rate = clamp(osc_rate,.125,8)`; `phase0 = frac(phase + seed×.5 + variation×.071)`. Wave value at `frac(i/n×rate + phase0)`, 8 shapes. Hits are a Euclidean placement `euclid_on(i,wanted,n)` — not the waveform peaks. | Default pitch using the wave as `u`; `osc=(rate,phase0)`; accent `.55+.45×wave`. | _Nelody/src/sequence/oscillator.rs:34 generate; :6 value; _Nelody/src/sequence/euclidean.rs:3 requested_hits; _Nelody/src/sequence/mod.rs:270 euclid_on |
| 10 | L-System Grammar (Rhythm) | `pulses`, `density`, `seed`, `phase`, `variation`, `symmetry` | `thr = clamp((pulses/n)×(.35+.65d),0,1)`. `symmetry` 1/2/3/5 selects family; otherwise `(floor(seed×4)+variation)%4`. 16 parallel rewrites into 1024 symbols. Contour walk starts `frac(seed+phase)`; A adds `φ⁻¹×.5`, B subtracts `φ⁻¹×.35`, folded into `0..1`. `chance = thr × gw(symbol) × metrical(step)`, `gw(A)=1`, `gw(B)=.68`. | Default pitch from the contour. `lsym` = A/B word; `orbit` = chance; accent `.95` A, `.72` B. `n==1` or full density/pulses forces a hit. | _Nelody/src/sequence/lsystem.rs:24 generate; :7 threshold; :17 step_chance; :13 metrical_weight |
| 11 | Xenakis Sieve (Number Theory) | `ca_rule` (moduli), `phase`/`variation` (residues), `seed`, `pulses` (terms), `density` | `MODULI=[2,3,5,7,11,13]`; `m1=MODULI[ca_rule%6]`, `m2=MODULI[(ca_rule/6)%6]`, `m3=MODULI[(ca_rule/36+3)%6]`. `shift=round(phase×6)+variation`; `r1=(1+shift)%m1`, `r2=(2+shift)%m2`, `r3=(3+shift+floor(seed×97))%m3`. `terms=2 if pulses>=2 else1`; member `= (c1 OR (terms==2 AND c2)) AND NOT c3`. Slot `= ((i/m1 + (c2?3:0)) + shift) % 7`. `wanted=round(candidates×d).max(1)`; `top_k` on member score `1+.25×depth+jitter`. | Slotted pitch (`slots`, `u=(slot+.5)/7`); accent `.6+.25×score`; `sieve_cls` published. | _Nelody/src/sequence/sieve.rs:31 generate; :6 classes; :18 class_contains; :26 is_member; _Nelody/src/sequence/mod.rs:360 top_k |
| 12 | Fibonacci / Sturmian (Number Theory) | `pulses` (gate), `density`, `seed`, `phase`, `variation` | `alpha = clamp(d,.05,1)×φ⁻¹`. `phase0 = frac(seed+phase+variation×φ⁻¹)`. Onset when `floor((i+1)α) != floor(iα)` or `i==0`. `u = frac(phase0 + onset_index×φ⁻¹)`. | Default pitch. Accent `.7+.3×rng`; `map_param=alpha`; `orbit=u`. Density is the slope, not just a gate. | _Nelody/src/sequence/fibonacci.rs:9 generate; :4 alpha |
| 13 | Bernoulli (Stochastic) | `pulses` (gate), `density`, `seed`/`variation` | `prob=d`. Hit if `n==1`, `d>=.999`, or `rng<d`; pitch draw independent. | Default pitch (`u` = random draw); accent `.6+.4×rng`; fallback hit 0. | _Nelody/src/sequence/bernoulli.rs:4 generate |
| 14 | Acid Random (Acid) | `pulses` (note count), `density` (root bias), `accent`, `slide`, `octave`, `seed`/`variation` | `count=clamp(pulses,1,n)`. Per-step key `= rng + downbeat bonus (i%4==0 .3, i%2==0 .12, else 0)`; step 0 forced to 9.0. Sort descending; top `count` become hits. `w=acid_weights(d)`; each note draws slot from `w`, accent `< accent`, slide `< slide`, octave via `oct_from`. | Acid pitch path. `AcidLine` → `u=(slot+.5)/7`, `state=slot+1`, `acc=.95` flagged else `.76`. | _Nelody/src/sequence/acid_random.rs:5 generate; _Nelody/src/sequence/mod.rs:305 acid_weights; :311 oct_from; :284 pick; :348 acid_commit |
| 15 | Acid Euclid (Acid) | `pulses` (note count), `density` (root bias), `accent`, `slide`, `octave`, `seed`/`variation` | Notes are `E(count,n)` with `euclid_on`. Three cell degrees drawn once, cell 0 forced to 0. Nested Euclidean flags over note index `j`: `k = round(param×count)`, onset if `k>0 && ((j+offset)×k)%count < k`. | Acid pitch path. `AcidLine` commit as above. | _Nelody/src/sequence/acid_euclid.rs:5 generate; _Nelody/src/sequence/mod.rs:270 euclid_on; :305 acid_weights; :284 pick; :348 acid_commit |
| 16 | Acid Markov (Acid) | `pulses` (stationary note density), `density` (leapiness), `accent`, `slide`, `octave`, `seed`/`variation` | `d = clamp(pulses/n,.05,1)`. Two-state chain: if sounding, stay sounding with `r0 >= (1−d)×.5`; if resting, start with `r0 < d×.5`. `prox_exp = 3×(1−density)`. Pitch weights `ACID_W[b]×(1/(1+abs(b−prev)))^prox_exp`. Accent chance `accent×(1.5 if leap else .8)`; slide pass uses next hit and multiplier `1.6` for interval ≤2, else `.4`. | Acid pitch path. `AcidLine` commit. | _Nelody/src/sequence/acid_markov.rs:5 generate; _Nelody/src/sequence/mod.rs:320 next_hit_in; :311 oct_from; :284 pick; :348 acid_commit |
| 17 | Acid Motif (Acid) | `pulses` (motif length), `density` (gate), `phase` (mutation), `accent`, `slide`, `octave`, `seed`/`variation` | `l=clamp(pulses,2,16)`, limited to `n`. Motif hit `j==0 or rng<.7`; random slots with fixed `acid_weights(.8)`; flags from knobs. Repeat motif over the pattern. From repeat 2 on, mutation chance `phase×.6`: 25% toggle hit (not step 0) and reslot, 55% reslot, otherwise toggle octave. | Acid pitch path. `AcidLine` commit. Density only gates. | _Nelody/src/sequence/acid_motif.rs:5 generate; _Nelody/src/sequence/mod.rs:305 acid_weights; :311 oct_from; :284 pick; :348 acid_commit |
| 18 | Acid Shift Register (Acid) | `pulses` (register length), `density` (gate), `phase` (feedback), `accent`, `slide`, `octave`, `seed`/`variation` | `len=clamp(pulses,4,16)`; LFSR masked to `len`, forced nonzero. Each step: feedback = top bit, flipped with probability `phase×.5`; shift register. Fields: hit = bits 3..4 ≠ 0; slot = `SHIFT_DEG[bits 0..2]`; accent = bits 5..8 < `round(accent×16)`; slide = bits 9..12 < `round(slide×16)`; octave = bits 13..15 < `round(octave×8)`. | Acid pitch path. `lsym` = low register byte; fallback hit 0. | _Nelody/src/sequence/acid_shift.rs:7 generate; _Nelody/src/sequence/mod.rs:348 acid_commit |
| 19 | Thue-Morse (Number Theory) | `pulses` (target), `density` (keep), `phase`, `variation`, `seed` | `wanted=round(min(pulses,n)×d).max(1)`. `offset=round(phase×16)+variation+round(seed×64)`. Word bit = parity of popcount. Contour = Nørgård infinity series `((infinity(i+offset)+5)/10).clamp(0,1)`. `select_spread` picks `wanted` from the 1s. | Default pitch from `u`. `lsym`/`state`/`orbit` = bit; accent `.7+.3×bit`. Single line; no per-lane windows. | _Nelody/src/sequence/thue_morse.rs:22 generate; :4 bit; :11 infinity; _Nelody/src/sequence/mod.rs:389 select_spread |
| 20 | Rudin-Shapiro (Number Theory) | `pulses` (target), `density` (keep), `phase`, `variation`, `seed` | `wanted` and `offset` as Thue-Morse. Bit = parity of overlapping `11` pairs. `slot=(i+bit)%7`; `u=(slot+.5)/7`; `select_spread` from 1s. | Slotted pitch. `lsym`/`state`/`orbit` = bit; accent `.7+.3×bit`. | _Nelody/src/sequence/rudin_shapiro.rs:14 generate; :4 bit; _Nelody/src/sequence/mod.rs:389 select_spread |
| 21 | Brownian Walk (Stochastic) | `pulses` (target), `density` (σ), `phase` (start), `seed`/`variation` | `wanted=round(min(pulses,n)×d).max(1)`. `σ=.01+.2d`; `x=frac(.5+phase)`. Gaussian step via Irwin-Hall(4) × σ; fold into `0..1`. Score = `abs(step)` with step 0 forced to 1.0; `top_k`. | Default pitch from walk `u`. Accent `.62+.38x`; `orbit=u`. | _Nelody/src/sequence/brownian.rs:16 generate; :4 gaussian; :10 sigma; _Nelody/src/sequence/mod.rs:371 fold01; :360 top_k |
| 22 | Lévy Flight (Stochastic) | `pulses` (target), `density` (tail/scale), `phase` (start), `seed`/`variation` | `wanted` as Brownian. `α=2−1.5d`; `s_min=.02+.06d`; `len=min(s_min×U^(−1/α),8)`; random sign; fold. Score = `len` with step 0 forced 1.0; `top_k`. | Default pitch from walk `u`. Accent `.58+.42x`; `orbit=u`. | _Nelody/src/sequence/levy_flight.rs:22 generate; :4 tail_alpha; :9 step_min; :15 step_len; _Nelody/src/sequence/mod.rs:371 fold01; :360 top_k |
| 23 | Tent Map (Chaos / Physics) | `pulses` (target), `density` (μ), `seed`, `phase`, `variation` | `wanted=round(min(pulses,n)×d).max(1)`. `μ=1+.99d`; `f(x)=μ×min(x,1−x)`. `x=.05+.9×frac(seed+phase+variation×φ⁻¹)`; 32 burn-in steps. Scores = orbit `x` (step 0 forced 2.0); `top_k`. `u=normalize01(orbit[(i+delay)%n], observed lo/hi)`, delay min 3. | Default pitch. Accent `.62+.38u`; `map_param=μ`. | _Nelody/src/sequence/tent_map.rs:11 generate; :4 mu_of; _Nelody/src/sequence/mod.rs:377 pitch_delay; :382 normalize01; :360 top_k |
| 24 | Hénon Map (Chaos / Physics) | `pulses` (target), `density` (a), `seed`, `phase`, `variation` | `wanted` as Tent. `a=1+.4d`, `b=.3`. Start `x=(frac(seed×φ+phase+variation×φ⁻¹)−.5)×.4`, `y=(frac(seed+variation×(1−φ⁻¹)+phase×φ)−.5)×.2`; 64 burn-in steps. Score `= abs(x−prev_x)` (step 0 forced 1.0); `top_k`. | Default pitch from y (`theta`). `u=normalize01(theta over hit set)`; accent `.58+.42×ux`; `map_param=a`. | _Nelody/src/sequence/henon_map.rs:11 generate; :4 a_of; _Nelody/src/sequence/mod.rs:382 normalize01; :360 top_k |
| 25 | Arpeggiator (Music Theory) | `pulses` (chord tones), `density` (gate), `phase` (direction), `seed`/`variation` | `size=clamp(pulses,1,4)`; chord slots `[0,2,4,6]` (1-3-5-7). `dir=floor(phase×4)%4`: up, down, up-down, random. `cycle=2×size−2` (or 1 when `size=1`). `oct=(i/(size×4))%3`. Hit on `i==0` or `rng<density`. | Acid pitch path. `u=(slot+.5)/7`, `state=slot+1`, accent `1.0` downbeat else `.78`; `acid=true`. | _Nelody/src/sequence/arpeggiator.rs:11 generate |
| 26 | Walking Bass (Music Theory) | `pulses` (chord tones), `density` (approach chance), `phase` (side), `seed`/`variation` | `size=clamp(pulses,1,4)`; chord `[0,4,7,10]`; ping-pong `walk_index`. Even steps: chord tone, always hit. Odd steps: next chord tone plus chromatic approach with probability `density`; `dir=floor(phase×3)%3`: below, above, mixed. | Absolute pitch (`semis`). `u=(semi+12)/24`; accent `1.0` downbeat else `.72`. | _Nelody/src/sequence/walking_bass.rs:15 generate; :6 walk_index |
| 27 | Hilbert Curve (Geometric) | `pulses` (order), `density` (gate), `phase` (turns), `seed`/`variation` | `order=clamp(pulses,1,5)`; `total=4^order`; `d=i×total/n`; Hilbert `point(order,d)`; rotate by `floor(phase×4)` quarter turns. `hit = i==0 or rng<density`. | Default pitch from x. `orbit=ux`, `theta=uy`, `state=(x×size+y)%256`; accent `.62+.38ux`; `nv=order`. | _Nelody/src/sequence/hilbert_curve.rs:29 generate; :3 point |
| 28 | Ulam Spiral Primes (Number Theory) | `pulses` (target), `density` (keep), `phase`, `variation`, `seed` | `wanted=round(min(pulses,n)×d).max(1)`. `offset=round(phase×64)+5×variation+round(seed×64)`; `cell=1+offset+i`. Square spiral walk; prime cells (including 1) are primaries. `select_spread` picks `wanted` primes. | Default pitch from `(x+y)/2`. `orbit`/`theta` normalized coordinates; `state` = prime flag; accent `.95` hit else `.7`; `nv=side`. | _Nelody/src/sequence/ulam_primes.rs:23 generate; :3 is_prime; _Nelody/src/sequence/mod.rs:389 select_spread |
| 29 | Life-like CA (Cellular) | `pulses` (warm-up), `density` (cell density + gate), `ca_rule`, `symmetry`, `seed`/`variation` | Fixed 16×8 torus. `ca_rule%5` selects Life/HighLife/Seeds/Day & Night/dense rule. `symmetry` selects centre block, stripes, sparse random, denser random; a diagonal live cell guarantees every row. `pulses=min(64,pulses)` warm-up generations (after the global `min(pulses, steps)` clamp). Each step reads column `i%16`; live rows form an 8-bit mask; `turn=i%4`; `code=rotated 4-bit window`; `slot=code%8`. Gate `=(.3+.7d)×(.45+.55×population)`; hit if `steps==1` or population > 0 and (`d>=.99` or local `< gate`). | Slotted pitch. `u=(slot+.5)/8`, `orbit=population`, accent `.65+.35×population`; publishes final `ca` grid. | _Nelody/src/sequence/life.rs:105 generate; :11 rule_masks; :30 seed; :72 step |

---

## 3. Shared post-generation pipeline

1. **Input clamps:** `steps` 1..1024; `pulses` to `steps` (Cellular to 15); `rotate %= steps`; `symmetry` 1..8; `osc_wave %= 8`.
2. **Generator:** `run_mode(&q)` produces `hit`, `u`, `acc`, `oct`, `slide`, `semis`, flags (`absolute`, `acid`, `slotted`), symbols and view data.
3. **Acid opt-in:** `out.acid |= q.kind.is_acid()`. Markov also sets it itself.
4. **Probability:** one per-step draw with salt `0xBABA_0000 + i`. Dropping a hit clears `slide[i]`.
5. **Hard mute:** `density <= 0` or non-Cellular/Life `pulses == 0` clears hits and slides.
6. **Rotation:** `shift = (rotate % steps + variation % steps) % steps`; reorders `hit`, `u`, `acc`, `oct`, `slide`, `semis`, `state`, `slots`, `orbit`, `orbit_next`, `theta`, `lsym`, `step_edge`, `tri`, `roll`, and `ca`.
7. **Invert:** for non-absolute lines, resting steps with `u == 0.0` get a contour continuation `frac(last + 1 − φ⁻¹)` before the hit/rest flip; any new rest clears `slide`.
8. **Mirror:** reverses every per-step array and sets `Pattern.mirrored`.
9. **Metric accents:** generator accents are multiplied by the metric layer (`i%4==0 → 1.0`, `i%2==0 → .82`, else `.68`) with a seeded human factor. Acid lines use a reduced `±1.5%` shimmer (`0.985 + 0.015×acc`) so the flag and velocity agree.
10. **Manual edits:** display-space hit/accent override; probability is used in the global pass so it can restore a hit. Edits win over generator/metric accents.
11. **Scheduler:** `step_index(k, length)` maps the cursor to a cell; swing delays odd steps; ratchets divide a step into 12 substeps (1..4 supported); gate and microtiming come from the edit overlay or globals; pitch is computed at the resolved onset.

### Nelody probability note

Nelody generators use `density` for their own probabilistic gates, but **none of them reads `probability`**. The global pass is the only `probability` application. There is no nDrums-style double-probability behavior here.

---

## 4. Pitch, tuning and note transfer

`pitch_for_values()` selects one of four paths:

| `Pattern` flag | Formula | Modes |
|---|---|---|
| `absolute` | `semis = pattern.semis[idx] + pattern.oct[idx]×12`; `cents = root_cents` | Tonnetz, Walking Bass |
| `slotted` | `Tuning::pitch_slot(pattern.slots[idx], pattern.oct[idx], edo, root_cents)`; slot is a degree inside one octave; `oct` shifts whole octaves | Eulerian, Sieve, Rudin-Shapiro, Life |
| `acid` | `Tuning::pitch_in_octave(pattern.u[idx], pattern.oct[idx], edo, root_cents)`; `u` spans one octave; `oct` shifts | Acid modes, Markov, Arpeggiator |
| default | `Tuning::pitch(pattern.u[idx], octaves, edo, root_cents)`; `u` spans `octaves` octaves | All others |

Then: `midi_note = clamp(midi_root + root + semis, 0, 127)`, `cents` is the residual detune.

### Tuning math

- `Tuning::degrees(octaves, edo)` = table length × octaves, or `edo × octaves` for `Tuning::Edo`.
- `degree_cents(deg, edo)` = table cents for `deg % table_len` + `1200 × (deg / table_len)`, or `(deg % edo)×1200/edo + 1200×floor(deg/edo)`.
- `pitch(u, octaves, edo, root_cents)`: quantizes `u` to a degree, adds `root_cents`, rounds to the nearest semitone, and returns `(semis, cents_remainder)`. Detune is always within ±50 cents.
- `pitch_slot(slot, oct, edo, root_cents)`: same, but slot is within one octave and `oct` is added as `1200×oct`.
- `pitch_in_octave(u, oct, edo, root_cents)`: maps `u` to a slot inside one octave, then calls `pitch_slot`.
- `Tuning::Edo` has no table; its step is `1200/edo` cents. `MAX_EDO = 72`.
- `root_cents` is added before quantization, so ±50 cents can tip a degree onto the neighbouring semitone while preserving total pitch.

### MIDI / voice transfer

- `bend_value(cents, range_cents) = clamp(.5 + cents/(2×range_cents), 0, 1)`. `range_cents = bend_range×100`; missing/broken range defaults to ±200 cents.
- `nts` gates pitch bend: off sends no bend and resets any outstanding bend to centre.
- `bend_range` changes send RPN 0 (CC101=0, CC100=0, CC6=range, CC38=0, then RPN deselect) before the first note.
- `slide` notes hold until the next onset + `SLIDE_OVERLAP = 0.5` step; non-slide notes release after `gate × step_len`. Per-step length overrides the gate.
- Voice transitions: `Fresh` (nothing active), `Retrigger` (active note was not sliding), `Legato` (sliding into a different note), `Tie` (sliding into the same note; no new NoteOn).
- MIDI channel is 0 (MIDI channel 1); all notes are one melodic voice.

---

## 5. LFO target transfer (Nelody)

`ModTarget::ALL` has 24 targets. Links are persisted as one bit per target in `lfo*_links`; the old `lfo1_target`/`lfo2_target` enums are a legacy fallback only when the new mask is empty.

| Group | Targets | Transfer |
|---|---|---|
| Generation | `Steps`, `Pulses`, `Rotate`, `Density`, `Seed`, `OscPhase`, `OscRate`, `CaRule`, `Probability`, `Slide`, `Accent`, `OctaveJump`, `MaxSteps` | Sampled once per loop; causes `generate_with()` to run again. `MaxSteps` is resolved first. |
| Scheduling | `Swing`, `Gate`, `Velocity`, `RegenerateEvery`, `RandomizeAfter` | Sampled per note/block. `RegenerateEvery`/`RandomizeAfter` only work when their base parameter is non-zero (`automatic_interval` hard-off at 0). |
| Pitch | `Root`, `Octaves`, `MidiRoot`, `Edo`, `RootCents` | Sampled per note through `pitch_values_at()`; no regeneration. |
| View | `HistoryBars` | Editor/history window only. |

Signal: wave (8 shapes), rate mode (Hz or beats), frequency/rate, depth 0..2, phase, polarity (bipolar/unipolar/unipolar negative). `lfo_signal = depth × shaped_wave`, clamped ±2. `fold(base, offset, min, max)` adds `offset × (max−min)` to the base and clamps; integer targets round after summing all linked LFOs.

`gen_params_at()` applies the generation group only; it deliberately does **not** apply pitch targets to `GenParams`. `pitch_values_at()` applies the pitch group separately, so a tuning LFO changes notes without rebuilding the line.

---

## 6. Mode step floors

`Mode::min_steps()` is applied by `Mode::steps_for_mode()` when selecting a mode. It raises `steps` and, if needed, `max_steps`; it never lowers a longer loop or higher ceiling.

| Floor | Modes |
|---:|---|
| 32 | Logistic, L-System, Thue-Morse, Rudin-Shapiro, Brownian, Lévy Flight, Tent Map, Fibonacci |
| 64 | Hénon Map, Ulam Spiral Primes, Life-like CA |
| none | Euclidean, Golomb, Eulerian, Circle Map, Dice, Tonnetz, Cellular, Markov, Oscillator, Sieve, Bernoulli, Acid Random, Acid Euclid, Acid Markov, Acid Motif, Acid Shift, Arpeggiator, Walking Bass, Hilbert Curve |

---

## 7. Calculation gotchas worth knowing

- **`length` never reaches a generator.** Only `steps` does. Fractional loops are scheduler wrapping only.
- **`pulses == 0` is a global mute** for every mode except Cellular and Life, even when a mode does not use `pulses` internally (Fibonacci, Bernoulli, Dice and others still go silent).
- **`density <= 0` is a global mute.** In chaos modes density is primarily a map coefficient, so zero means both “parameter 0” and “silence”.
- **`rotate` is composed with `variation`.** Generate can move the line even when the generator itself is deterministic in `seed`.
- **`seed` is sometimes hidden but still affects accents.** Modes that ignore seed for the line still use it in the shared accent humanizer.
- **`accent` is not velocity.** It is an acid flag probability; the actual multiplier is `.95`/`.76` in `Pattern.acc`, then the metric shimmer and `velocity`.
- **`pulses` is overworked.** It is note count, ruler order, chord size, motif length, register length, CA row, walk length, warm-up count or order. Use the table above, not the generic label.
- **`phase` is overworked.** It is map start, word offset, mutation probability, register flip probability, direction, approach side, CA row offset or curve turn.
- **`mode_data` is currently unused.** `GenParams::mode_data = [0.0; 4]` is reserved for typed future parameters; no generator reads it yet.
- **`slotted`/`acid`/`absolute` flags are the pitch contract.** Changing which flag a generator sets changes the pitch path without changing `u`, so test both the generator and `pitch_for_values()`.
- **Manual edits are display-space.** They apply after rotation/mirror; probability overrides are evaluated in the same global pass as `probability`, so an override can restore a globally dropped hit.
