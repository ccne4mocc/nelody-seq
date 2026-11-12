---
name: nelody seq
description: Algorithmic MIDI melody with the tuning and the path visible on screen
colors:
  lime-signal: "#bef264"
  chord-green: "#22c55e"
  teal-third: "#0d9488"
  focus-orange: "#fb923c"
  chamber-deep: "#0d1712"
  chamber-moss: "#111b15"
  chamber-teal: "#0e1b19"
  surface: "#101e17"
  control: "#1b2c23"
  control-hover: "#2b4033"
  text: "#e3ece5"
  text-muted: "#93a89b"
  border: "#354b3e"
  cell-off: "#16261d"
  on-accent: "#0b0f14"
typography:
  display:
    fontFamily: "system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 700
    letterSpacing: "0.5px"
  title:
    fontFamily: "system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 700
  body:
    fontFamily: "system-ui, sans-serif"
    fontSize: "12px"
  value:
    fontFamily: "system-ui, sans-serif"
    fontSize: "11px"
  caption:
    fontFamily: "system-ui, sans-serif"
    fontSize: "10px"
  dense:
    fontFamily: "system-ui, sans-serif"
    fontSize: "9px"
rounded:
  tick: "1px"
  chip: "3px"
  control: "4px"
  action: "5px"
  handle: "11px"
spacing:
  hairline: "3px"
  tight: "5px"
  row: "6px"
  panel: "8px"
  section: "10px"
  gutter: "12px"
components:
  action-primary:
    backgroundColor: "{colors.focus-orange}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.action}"
    typography: "{typography.value}"
    width: "112px"
    height: "30px"
  action-secondary:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    rounded: "{rounded.action}"
    typography: "{typography.value}"
    width: "112px"
    height: "30px"
  chip-toggle:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    rounded: "{rounded.chip}"
    typography: "{typography.caption}"
    height: "24px"
  tuning-chip:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    typography: "{typography.value}"
    height: "28px"
  slider-row:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    typography: "{typography.value}"
    height: "25px"
  card-panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
  field-input:
    backgroundColor: "{colors.control}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    typography: "{typography.value}"
    height: "25px"
---

# Design System: nelody seq

## Overview

**Creative North Star: "The Signal Observatory"**

nelody seq watches a melody walk. The Tonnetz diamond, the pitch roll and the tuning strip are the
observable objects; the surrounding chrome is the housing that keeps them readable. Where its
percussive sibling draws a rosette of pulses, this one draws a path through harmonic space and a
history of the notes it produced. Both share one chamber, one type system and one control
language, because they are two instruments in the same rack.

The surface is sparser than nrum seq by geometry: a fixed 860x697 window, one drawing, six
parameter rows, a tuning strip and a roll that is allowed to hold real air. That air is
intentional. The roll is where the user reads the melody, so it gets the largest single region and
the fewest marks inside it.

**Key Characteristics:**
- Green chamber with a three-hue signal ramp and a single warm focus accent.
- Fixed 860x697 coordinate space; the roll is the largest region and stays uncluttered.
- The same job-named type scale as nrum seq, 9-14px, weight and tone carrying hierarchy.
- Flat surfaces; depth from tonal layering, shadows only on overlays.
- Two authored differentiators: the Tonnetz drawing and the tuning strip.

## Colors

A moss-dark chamber, a green-to-teal signal ramp, and one warm accent reserved for attention.

### Primary
- **Lime Signal** (#bef264): the first accent and the note body colour. The history roll draws
  every note in it at 0.8 alpha, so it is the colour of "this is the melody".
- **Chord Green** (#22c55e): the second accent. The Tonnetz edges and the active tuning state.
- **Teal Third** (#0d9488): the third accent. Thirds and harmonic distance in the drawing, and the
  far stop of the parameter fill ramp.

### Neutral
- **Chamber Deep** (#0d1712): the window behind everything.
- **Chamber Moss / Teal** (#111b15, #0e1b19): alternate chamber tones that separate the drawing
  field from the control field.
- **Surface** (#101e17): cards, menus, drawers.
- **Control** (#1b2c23): the resting fill of every interactive row and chip.
- **Control Hover** (#2b4033): the only hover fill.
- **Border** (#354b3e): 1px separators and card edges, deliberately low-contrast.
- **Text** (#e3ece5) and **Text Muted** (#93a89b): the two text tones.
- **Cell Off** (#16261d): empty roll and matrix ground.
- **On Accent** (#0b0f14): the only text colour permitted on a saturated fill.
- **Focus Orange** (#fb923c): the focus ring, the playhead and the primary action. A state colour,
  not a brand colour.

### Named Rules
**The One Reading Rule.** At rest, a screen carries the roll in lime, the drawing in green and teal,
and one primary action in focus orange. Everything else is neutral.

**The Signal Not Decor Rule.** A saturated colour encodes a note, a harmonic relationship, or a
state the user can act on. Otherwise it is a bug.

**The Solid State Rule.** A state fill is the accent at full opacity. A tint behind a label drops
it below 4.5:1 in at least one theme; the tint is the defect, not the hue.

## Typography

**Display Font:** system UI sans (no font asset ships; the family deliberately falls back)
**Body Font:** system UI sans
**Label Font:** the same face, differentiated by weight, case and 0.5px tracking

**Character:** One system face at eight sizes. Headings are uppercase, bold and tracked; note and
tuning values are full-tone; support copy is muted. The wordmark is 24px/700 with 0.5px tracking.

### Hierarchy
- **Display** (700, 24px, 0.5px tracking): the wordmark only.
- **Title** (700, 14px): panel headings and the active generator name.
- **Body** (400, 12px): generator descriptions and settings explanations.
- **Value** (400, 11px): parameter values, option labels, tuning readouts, primary action labels.
- **Caption** (400-700, 10px, uppercase when a heading): chips, controls, status, column headers.
- **Dense** (400, 9px): the pitch ruler and four-digit badges.

### Named Rules
**The Two-Tone Rule.** A label and its value share a row and differ in tone, not size.

**The 9px Floor Rule.** 9px is for labels inside geometry that cannot grow — the pitch ruler above
all. A control label at 9px is a layout failure.

## Layout

A fixed 860x697 canvas. The wordmark and transport close the top; a 300px drawing panel sits beside
the parameter column; below them the history roll takes the remaining height; the tuning strip
sits inside the parameter column, above the roll; the status strip closes the bottom.

The spacing rhythm is 3/5/6/8/10/12, identical to nrum seq. Five to six between controls in a row,
eight to ten between groups, twelve at panel padding. There are no breakpoints; the host supplies
the window at this size.

The parameter column is the one place that changes height per mode: a mode may add a row (the
Markov bias, the symmetry control, the acid trio), and the flexible spacer above the tuning strip
absorbs it. The roll absorbs everything else.

## Elevation & Depth

Depth is tonal like its sibling: chamber → surface → control → control-hover, with no shadow
inside the panel. Overlays float on one of two shadow sizes.

### Shadow Vocabulary
- **Popover** (`drop-shadow-blur: 12px; drop-shadow-color: #00000055; drop-shadow-offset-y: 3px`):
  anchored menus, the waveform picker, the scale dropdown.
- **Drawer** (`drop-shadow-blur: 16px; drop-shadow-color: #00000055; drop-shadow-offset-y: 4px`):
  the settings panel and the tuning drawer.
- **Scrim** (`#00000044` popover, `#00000066` drawer): the dimming layer under a modal surface.

### Named Rules
**The Flat-At-Rest Rule.** No surface inside the panel carries a shadow.

## Shapes

1px for ticks, 3px for chips and cells, 4px for rows, inputs and cards, 5px for the two primary
actions, 11px for a circular drag point. Borders are 1px and low-contrast; a control is identified
by fill, not outline.

The recurring silhouette is again the horizontal slot with a left-anchored fill — the parameter
row, the tuning chip, the roll's note bars. The family's other shape is the scroll: a wide band
that holds a drawing and nothing else.

## Components

### Buttons
- **Shape:** 5px radius, 30px high, 112px wide for the two actions.
- **Primary:** focus orange fill, on-accent text, 11px/700. Exactly one on screen: GENERATE.
- **Secondary:** control fill, full text, 11px/400, same size. RANDOMIZE sits beside it.
- **Hover / Focus:** hover swaps to control-hover; the primary gains a 2px accent border. Focus is
  a 1px inset ring in focus orange.

### Chips
- **Style:** 24px high, 3px radius, control fill, caption text. Toggle chips fill with the state
  colour and switch text to on-accent.
- **State:** off is neutral; on is a solid accent fill at full opacity with on-accent text.
  State is never shown with a border alone.

### Cards / Containers
- **Corner Style:** 4px.
- **Background:** surface.
- **Border:** 1px border at low contrast.
- **Internal Padding:** 5-10px.
- **Shadow Strategy:** none at rest.

### Inputs / Fields
- **Style:** control fill, 1px border, 4px radius, 25-28px high, value right-aligned.
- **Focus:** a 1px focus-orange ring on the row; text entry replaces the row interior.
- **Error / Disabled:** muted text; no dedicated error style.

### Tuning Strip (signature)
A single row carrying root pitch class, scale, octave and NTS. Root and octave are steppers, scale
is a dropdown chip, NTS is a toggle. This is the product's authored tuning model, and the row is
the one place where three control grammars meet — the reason it must share one height, one radius
and one label tone.

### History Roll (signature)
A wide band of pitch against time, drawn as filled bars: width encodes gate or length, brightness
encodes velocity or accent, outline encodes slide. The playhead is a vertical focus-orange line.
The roll is where the melody is read; keep its ground empty.

## Do's and Don'ts

### Do:
- **Do** keep saturated colour for notes, harmonic relationships and state only.
- **Do** place controls in the fixed 860x697 space rather than adding breakpoints to the main panel.
- **Do** use 3-4px radii, 1px low-contrast borders, and fills to indicate identity.
- **Do** let the roll hold real air; it is read, not filled.
- **Do** keep the tuning strip's four controls on one height and one label tone.

### Don't:
- **Don't** use gradients as fills, glows, or glass. The one gradient is the accent ramp
  (Lime → Chord Green → Teal) that marks a selected row, parameter or tuning chip; it never
  appears as decoration.
- **Don't** build a dark web dashboard: no purple, no translucent cards, no pills, no emoji labels.
- **Don't** imitate a generic DAW: no brushed metal, no knobs, no tabbed preferences, no stock
  widget styling.
- **Don't** build a sci-fi HUD: no scanlines, no neon wireframes, no hex grids, no idle telemetry.
- **Don't** raise functional labels above 11px to create hierarchy; use weight and tone, and if
  that fails the layout is wrong.
