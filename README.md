# Crucible

A simple DAW with a clean interface. Rust audio engine, Tauri 2 shell, TypeScript UI.
Windows first. Hosts VST3 instruments, with Spectrasonics Omnisphere and Keyscape as the
reference targets. Designed so macOS and Linux can follow without a rewrite.

Small, fast and robust by design: plain data, few dependencies, measured budgets, and every
action available as a command (`crucible help`). The GUI is built on top of the same commands.

Status: early development (milestone M0). Nothing here has been through a listening test yet,
and the ASIO path has not run against a real driver. Start with `docs/STATUS.md`.

**Download:** the Windows installer is on the Releases page. It is not code-signed, so Windows
SmartScreen warns about an unknown publisher (More info, Run anyway).

**Build:** `docs/STATUS.md`, "How to build and test". You need Rust, Node.js and LLVM's
`libclang.dll`; everything else is in this repository.

Why build it: every existing DAW carries decades of accumulated options. This one keeps
only the essentials and puts sound quality and a calm interface first. It is a learning
project and a tool for a small group, not a commercial product.

See `CLAUDE.md` for the working rules and the doc map.

## Licence

Crucible is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It comes with no warranty. The full text is in
`LICENSE`.
