# 07. Hardware and testing

## The four machines

| Machine | Role | Audio | Driver path | Notes |
|---|---|---|---|---|
| **Dev workstation** | Where the code is written today | Realtek built-in, Jabra headset, Thunderbolt dock | None suitable | Lenovo work laptop. No ASIO interface, no MIDI keyboard, no VST3 plugins. Cannot run any hardware test. Test builds are copied from `dist/` to the music machines. |
| **Lynx machine** | Reference. Record and judge here. | Lynx PCI card, model not yet given | Lynx's own ASIO driver | Confirm Lynx still publishes a driver for that card on its Windows version. |
| **Behringer machine** | Second test target | External Behringer USB interface, model not yet given | Depends on the model. UMC HD models have a native ASIO driver. UM2, UMC22 and UCA models rely on the generic ASIO4ALL wrapper. | |
| **Laptop** | Arranging on the go | ASUS Vivobook S 16 Flip TN3604YA. Ryzen, 32 GB. Realtek ALC294, confirmed from the hardware ID `VEN_10EC&DEV_0294`. | No ASIO. WASAPI exclusive mode. | Ships with Dolby Atmos and speaker-flattering processing. Not for judging a mix. Plug the Behringer in for anything played or judged by ear. A Bluetooth speaker is paired with it. Never monitor through Bluetooth. Touchscreen. |

All test machines run Windows 10 or 11. Each has a different generic MIDI keyboard.
Exact interface models do not matter: the DAW uses whatever drivers are installed (D-018).

## What three machines force into the design

- Two Windows audio paths from the start: ASIO and WASAPI exclusive mode.
- Device and buffer settings are stored per machine, never in the project.
- MIDI records from all connected inputs by default. Nothing binds to a keyboard's name.
- Audio files sit inside the project folder and are referenced by relative path.
- Plugins are identified by class ID, not by path.
- Omnisphere and Keyscape must be installed with the same sound library on every machine.
  The project stores their settings, not their sounds.

## The open workflow question

The dev workstation cannot test audio, MIDI or plugins. Until that changes, every hardware test
needs a build carried to another machine and a report carried back. That loop sets the pace of
the whole project.

Options, for Dzyan to choose:

| Option | Loop speed | Cost |
|---|---|---|
| Develop directly on the Lynx machine | Fastest. Claude builds and Dzyan tests in the same session. | Needs Rust, Node and Claude Code installed there. |
| Develop here, copy builds across | Slow. One round trip per bug. | Nothing to install on the music machines. |
| Develop here, test on the laptop with the Behringer plugged in | Middle. The laptop can sit next to this workstation. | Covers two of the three driver paths, not the Lynx. |

Recorded as blocker 2 in `STATUS.md`.

## What a machine can verify and what it cannot

A coding session has no ears, no eyes on a plugin window, and no licensed plugins.

**Verifiable by machine, with no one present:**

- Code compiles. Unit tests pass.
- Every offline-render test in `04-audio-quality.md`: null test, determinism, gain and pan law.
- The audio callback does not allocate.
- Project files save, load and survive a simulated crash.
- Devices and plugins are enumerated correctly on the machine it runs on.
- A free VST3 instrument loads, processes audio and returns its state, once one is installed.
- The front end renders and holds its frame rate in a browser with fake data.

**Needs Dzyan:**

- Anything involving Omnisphere or Keyscape.
- Whether sound actually comes out, and whether it is clean.
- Latency by feel.
- Whether a plugin's editor looks and behaves right.
- Clicks, dropouts and drift during a long session.
- Whether the interface looks good.

Every session report keeps these two lists separate. See rule 3 in `CLAUDE.md`.

## How a hardware test is run

1. The session writes a checklist to `test-reports/` from `TEMPLATE.md`, naming the build to use.
2. Dzyan runs it on the named machine and fills in the results, or dictates them.
3. The filled-in report stays in the repo as the record.
4. A failure includes what he saw and heard, the buffer size, the device, and any log file the program wrote.

The program should write a plain-text diagnostic log on every run: devices found, driver in
use, sample rate, buffer size, plugins loaded, overruns counted. That log is the main way a
session learns what happened on a machine it cannot see.

## Plugin compatibility table

Filled in as plugins are tested. Empty until M0.

| Plugin | Version | Machine | Loads | Editor | Plays | State recall | Offline render | Notes |
|---|---|---|---|---|---|---|---|---|
| Omnisphere | | | | | | | | |
| Keyscape | | | | | | | | |

## Translation checks for finished music

Not software tests. These are the listening checks the DAW's tools are built to support.

1. Mix on the most neutral setup: the Lynx machine with studio monitors or reference
   headphones, all enhancement off.
2. Check on ordinary headphones and on basic powered speakers.
3. Press mono. Fix anything that disappears.
4. Compare with a commercial reference at matched loudness.
5. For live use, leave headroom and do not pre-limit.

## Platform notes

- The machine table is Windows-only today. Add rows when a Mac or Linux machine joins.
- The diagnostic log format should be identical on every platform so reports compare cleanly.
- On macOS the driver-path column disappears, since the built-in audio system is used.
- The reference plugins cannot be tested on Linux. A Linux test matrix needs its own reference
  instruments, for example Pianoteq, u-he synths or Surge XT.
