# Rules for any Claude session working in this folder

**Read `docs/STATUS.md` first.** It says what is done, what is next and what is blocked.
**Update `docs/STATUS.md` before you stop**, every session, even if little changed.

## What this is

A simple digital audio workstation (DAW). Rust engine, Tauri 2 shell, TypeScript interface.
Built for Dzyan, his family and a few fellow sound engineers. Not a mass-market product.
The product is called **Crucible** (D-023). Everything carries the name: the repository folder
`crucible`, the `crucible-*` crates, the `crucible` binary, the window `crucible-app` and the log folder.
This is a personal project. Nothing from Dzyan's employer belongs in this repo.

## Priorities, in order

1. **Clean, modern GUI with an uncluttered top bar.** Essentials only. Refusing features is the product.
2. **Transparent sound.** Bit-exact wherever the maths allows. No silent resampling, no hidden processing.
3. **Low latency** for playing virtual instruments live.
4. **Lossless export by default.**
5. **One project opens identically on three different Windows machines.**
6. **Windows first and perfected there, but never tied to Windows.** macOS and Linux come later.

When two of these conflict, the lower number wins. Details: `docs/01-vision-and-priorities.md`.

## Hard rules

1. **Platform seam rule.** Operating-system code lives only in the seam crates listed in
   `docs/03-platform-notes.md`. No `#[cfg(target_os = ...)]` and no Win32, WASAPI or ASIO call
   anywhere else. Every platform-specific file starts with a `// PLATFORM:` comment naming the
   macOS and Linux equivalent. Every new Windows-specific thing gets a row in the ledger in that doc.
2. **Real-time rule.** The audio callback never allocates, locks, logs, does file or network I/O,
   or waits on another thread. Details: `docs/02-architecture.md`.
3. **Verification rule.** You cannot hear audio, see a plugin window, or run Omnisphere.
   Dzyan is the tester. Every report separates **verified by machine** from **needs Dzyan's ears
   or hardware**. Never say audio works unless he confirmed it. Write his test steps into
   `docs/test-reports/` using the template there.
4. **Scope rule.** Build only what the current milestone in `docs/06-roadmap.md` names.
   A new feature idea goes into `docs/09-open-questions.md`, not into the code.
5. **Decision rule.** Decisions live in `docs/08-decisions.md`. Entries marked *Decided* are
   Dzyan's and are not re-argued. Entries marked *Proposed* are Claude's and still need his yes.
   New evidence against a decision gets appended to that entry and raised with him. It is not
   acted on silently.
6. **Licence rule.** Crucible is free software, `GPL-3.0-or-later` (D-036). Dependencies should be
   MIT, Apache-2.0, BSD, zlib or MPL. Anything else is raised first. Code may be ported from
   GPLv3-compatible projects (GPLv3, GPLv2-or-later, LGPL) when that is the better engineering:
   record the source and licence in `docs/10-references.md` and keep the original notice. Never
   from AGPL (Zrythm, JUCE 8), GPLv2-only or unlicensed code. The ASIO SDK is used under its
   GPLv3 option.
7. **Install rule.** Do not install toolchains, SDKs or plugins, and do not download anything
   outside normal `cargo` and `npm` dependency resolution, without Dzyan's yes in chat.
   Approved so far (2026-11-23): the ASIO SDK, LLVM and WiX, on the build machine (`10-references.md`).
8. **Git rule.** Commit only when Dzyan asks, unless `docs/STATUS.md` records a standing approval.
   Never push or create a remote without being asked.
9. **Docs rule.** Every design doc keeps a "Platform notes" section current. If code and docs
   disagree, fix the doc in the same session.
10. **Diagnostics rule.** Dzyan asked for errors and logs that anyone can diagnose. Every
   user-visible failure carries a code from `crates/crucible-diagnostics/src/codes.rs` and keeps the raw
   evidence (OS error number, path, device or plugin name) in its message. Programs start a session
   log and end with `crucible_diagnostics::finish`. New code means `crucible codes --markdown` regenerates the
   table in `docs/11-error-codes.md`. When Dzyan reports a problem, ask for `crucible report` output first.
11. **Lean rule** (D-024, details in `docs/01-vision-and-priorities.md`). Build it like id Software
   built Doom and Quake: plain data and plain functions, few dependencies, no layers that only
   pass calls along, measured budgets. Every user action is a command in a table (D-022); the GUI
   calls commands and never implements behaviour of its own. A change that breaks a budget is a bug.

## How Dzyan wants to be talked to

Short, plain, honest. Result first, then the next action. No salesmanship and no sugar-coating.
If something is fragile or was a wrong call, say so. Hand him raw PowerShell commands to paste.
He is new to Rust and to audio programming internals, so name the concept before the syntax.
He is an experienced sysadmin and reads TypeScript, Python, Go and C#.

**End every reply with "Your to-do and tests"** (Dzyan, 2026-11-23): everything waiting on him,
such as hardware tests, listening checks, decisions, and yes/no answers for installs, downloads or
commits. **Keep it very compact** (his words the same day: "very compact/condensed checklist"): one
short line per item, a file or command only where he needs it, no explanations, ten lines at most;
merge related items. Carry items over until he has done them. The full list lives in
`docs/STATUS.md` under "Blockers that need Dzyan"; keep the two in step.

**He asked to be caught.** When a GUI request (animation, effects, live previews) would cost frame
time or engine time, say what it costs, measured if possible, and offer a middle ground before
building it. The rules and budget are in `docs/05-gui-design-brief.md`, "Motion and effects".
He decides; record the outcome in `docs/08-decisions.md`.

## Doc map

| File | Purpose |
|---|---|
| `docs/STATUS.md` | Living handoff. Start here. |
| `docs/01-vision-and-priorities.md` | Why this exists, who it is for, what is in and out |
| `docs/02-architecture.md` | Layers, threads, the bridge, candidate crates, known hard problems |
| `docs/03-platform-notes.md` | Cross-platform ledger. The "never tied to Windows" contract |
| `docs/04-audio-quality.md` | Rules that keep the sound transparent, export defaults, headroom |
| `docs/05-gui-design-brief.md` | What "clean and modern" means here |
| `docs/06-roadmap.md` | Milestones with pass or fail criteria |
| `docs/07-hardware-and-testing.md` | The three machines, how testing works, what a machine can verify |
| `docs/08-decisions.md` | Decision log with reasons and rejected options |
| `docs/09-open-questions.md` | Things only Dzyan can answer, and unverified technical claims |
| `docs/10-references.md` | Sources, crates, reference code, licences |
| `docs/11-error-codes.md` | Error codes, where logs go, reading a crash, exit codes |
| `docs/12-known-daw-problems.md` | Problems other DAWs and hosts keep hitting, with sources, and what Crucible does about each. Read the relevant section before building drivers, plugins, MIDI, recording or export |
| `docs/13-reverb-and-delay.md` | What makes reverb, delay and echo sound right, with sources; the design of the built-in effects and their tests |
