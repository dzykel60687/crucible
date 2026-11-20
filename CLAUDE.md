# Rules for any Claude session working in this folder

**Read `docs/STATUS.md` first.** It says what is done, what is next and what is blocked.
**Update `docs/STATUS.md` before you stop**, every session, even if little changed.

## What this is

A simple digital audio workstation (DAW). Rust engine, Tauri 2 shell, TypeScript interface.
Built for Dzyan, his family and a few fellow sound engineers. Not a mass-market product.
The product is called **Crucible** (D-023). The folder stays `rust-daw` and the crates keep
their internal `daw-*` names; user-facing text, the `crucible` binary and the log folder use the name.
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
6. **Licence rule.** Do not copy or port code from GPL or AGPL projects (Ardour, Zrythm,
   Tracktion Engine, JUCE). Reading them to understand an approach is fine. Dependencies should be
   MIT, Apache-2.0, BSD, zlib or MPL. Anything else is raised first. The ASIO SDK has its own
   open licence question, see `docs/09-open-questions.md`.
7. **Install rule.** Do not install toolchains, SDKs or plugins, and do not download anything
   outside normal `cargo` and `npm` dependency resolution, without Dzyan's yes in chat.
8. **Git rule.** Commit only when Dzyan asks, unless `docs/STATUS.md` records a standing approval.
   Never push or create a remote without being asked.
9. **Docs rule.** Every design doc keeps a "Platform notes" section current. If code and docs
   disagree, fix the doc in the same session.
10. **Diagnostics rule.** Dzyan asked for errors and logs that anyone can diagnose. Every
   user-visible failure carries a code from `crates/daw-diagnostics/src/codes.rs` and keeps the raw
   evidence (OS error number, path, device or plugin name) in its message. Programs start a session
   log and end with `daw_diagnostics::finish`. New code means `crucible codes --markdown` regenerates the
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
