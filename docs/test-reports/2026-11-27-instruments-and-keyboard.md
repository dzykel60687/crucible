# Test steps: instruments and the MIDI keyboard (0.0.4)

For D-044. This is also the first test of plugin hosting with a real plugin (what M0 was for), so
anything odd matters. Do it after `2026-11-26-second-session-changes.md`.

## Setup

| Item | Value |
|---|---|
| Build | 0.0.4, `Crucible_0.0.4_x64_en-US.msi` |
| Machine | Home desktop: Behringer UMC202HD, M-Audio Axiom 61 |
| Plugins | Keyscape, plus any free VST3 instruments you want to try |
| Before starting | **Close Reaper.** Windows lets one program at a time have the keyboard |

## What a machine verified (dev laptop, no real plugin, no keyboard)

| # | Check | Result |
|---|---|---|
| 1 | The plugin folders are searched, each new plugin is asked what it is in a separate process, the list is kept and a second look asks nothing again | Pass (test instrument) |
| 2 | Adding an instrument track opens the picker; choosing loads the plugin and opens its window, floating above Crucible's | Pass (GUI test) |
| 3 | A key sounds in the very next audio buffer, through the device, and stops when released | Pass (unit test; GUI test through the laptop's headset) |
| 4 | Notes in a clip play exactly as the export writes them, sample for sample | Pass (unit test) |
| 5 | The sustain pedal reaches the plugin | Pass (test instrument) |
| 6 | Volume, pan, mute, solo act on the instrument; a key released while muted is not stuck afterwards | Pass (unit test) |
| 7 | The keyboard button moves the keyboards between tracks; a track can listen to one channel | Pass (unit and GUI tests) |
| 8 | The instrument's settings come back after save and open, and after undoing its removal | Pass (unit test) |
| 9 | A plugin missing on this PC is shown as "not installed" and its settings are kept | Pass (unit test) |
| 10 | 88 unit tests, 88 GUI checks | Pass |

Not verified: Keyscape or any real plugin, the Axiom, anything audible, how long Keyscape takes to
load.

## Needs your hands, ears and keyboard

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Settings, Audio: choose the UMC with ASIO (as in the other sheet) | The top bar shows the UMC | | |
| 2 | Add an instrument track (the "+ Instrument" button under the tracks) | A list of your instruments opens by itself; the first time it says it is looking. Keyscape is in it, and any free instruments you installed | | |
| 3 | Type part of a name in the search box | The list narrows | | |
| 4 | Choose Keyscape | Crucible may freeze a second or two while it loads. Keyscape's own window opens, in front of Crucible | | |
| 5 | Pick a piano in Keyscape's window and play the Axiom | Sound through the UMC, no delay you can feel. The keyboard button on the track (the small keys icon) is green and glows while you play | | |
| 6 | Sustain pedal, mod wheel, pitch bend (if Keyscape's patch uses them) | They act on the sound | | |
| 7 | Close Keyscape's window with its X. Click the track's instrument name, "Open its window" | It closes, and opens again with the same patch | | |
| 8 | Play hard for a minute: chords, fast runs | No clicks or dropouts. If you hear one, press F9 and type what you did | | |
| 9 | Add a second instrument track with another plugin (a free one). Click the keyboard button on each | The keyboard plays the track whose button is green. Ctrl+click lights both: both play | | |
| 10 | On the second track: its name, "Keyboard and channel", choose the Axiom and channel 2 (if the Axiom can send on channel 2) | It plays only from channel 2 | | |
| 11 | Draw a few notes in a MIDI clip on the Keyscape track (right-click the track lane, "New MIDI clip here", then the piano roll), press Space | They play through Keyscape at the right time | | |
| 12 | Export (Ctrl+Shift+E), play the file in Media Player | The notes are in the file, same sound | | |
| 13 | Change the patch in Keyscape, save the project (Ctrl+S), close Crucible, open the project again | Keyscape comes back with the patch you chose | | |
| 14 | Turn the volume of the Keyscape track down, mute it, solo the other | Acts on Keyscape as on any track | | |
| 15 | Unplug the Axiom, plug it back in, play | It plays again within a few seconds | | |
| 16 | Open Reaper while Crucible has an instrument, then look at the keyboard button's tooltip in Crucible | Expected: whichever opened the Axiom first has it. Tell me which one won | | |
| 17 | Try each free instrument you have: load, play, its window, save and reopen | Note any that fail to load, crash, show no window, or make no sound | | |

## Free instruments, if you want some to try

Any VST3 instrument works the same way. Common free ones: Surge XT, Vital, Dexed, Spitfire LABS.
Install them the usual way (into `C:\Program Files\Common Files\VST3`); the picker finds them the
next time it opens. A plugin elsewhere: "Add a folder…" in the picker.

## When a step fails

Press F9 at the moment it goes wrong and type what you noticed. Then Settings, Diagnostics,
Create report file, and send the file it names. If Crucible closes by itself while a plugin is
loaded, the log names the plugin's file that crashed: send it all the same.
