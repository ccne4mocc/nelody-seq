# Host verification

This file records what has actually been exercised. A host is **verified** only when the
checklist in `RELEASE_CHECKLIST.md` has been run against the current build on that host/API/OS
combination. Everything else is **untested**, even when the plug-in bundles and scans.

## Build under test

- Version: 1.3.1
- Bundles: `target/bundled/nelody-seq.clap`, `target/bundled/nelody-seq.vst3`
- Build command: `cargo xtask bundle nelody-seq --release`
- Date: 2026-11-03

## Automated checks

| Check | Result | Notes |
|---|---|---|
| `cargo test --lib` | **pass** | 183 passed, 0 ignored (2026-11-03). |
| Carla CLAP discovery | **pass** | `carla-discovery-native CLAP .clap` reports the expected name, label and 75 input parameters. |
| Carla VST3 discovery | **pass** | `carla-discovery-native VST3 target/bundled/nelody-seq.vst3` reports name `nelody seq`, label, maker and 75 input parameters. Requires the pinned nice-plug fork that defers activation until `setupProcessing()`. |
| CLAP validator | **pass** | `clap-validator 0.4.1`: **33 passed, 0 failed, 11 skipped**. The pinned nice-plug fork bounds malformed state lengths and rescans parameter values after a state load. |
| Shared-library resolution | **pass** | `ldd -r` on the `.clap` and VST3 `.so`: no unresolved symbols or missing libraries. Hard dependencies are libc/libm/libgcc/libfontconfig plus their transitive libraries. |
| VST3 validator | not run | `pluginval` is not installed on the test machine. |

## Manual host matrix

| Host | Version | OS | API | Status | Notes |
|---|---|---|---|---|---|
| REAPER | 7.83 | Linux x86_64 | CLAP | untested | Bundle builds; not yet scanned/loaded interactively. |
| REAPER | 7.83 | Linux x86_64 | VST3 | untested | |
| Bitwig Studio | installed | Linux x86_64 | CLAP | untested | |
| Carla | installed | Linux x86_64 | CLAP | untested | |
| Carla | installed | Linux x86_64 | VST3 | untested | Discovery passes; host load/editor/MIDI not exercised. |
| Windows | x64 | real or Wine | CLAP/VST3 | untested | Not cross-compiled or loaded. |
| macOS | — | macOS | CLAP/VST3 | untested | No macOS build has been produced or loaded. |

## Manual checklist per host/API

Run every row in `RELEASE_CHECKLIST.md` and record `pass`, `fail` or `n/a` with a note. Do not
promote a row to **verified** from a scan alone.

The Windows bundle workflow is manual-only. It cross-compiles MSVC artifacts on GitHub Actions,
but those artifacts are still unverified; no real Windows or Wine run has been recorded for this
plug-in. macOS is not configured.
