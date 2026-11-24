# 13. Reverb, delay and echo

Status: research notes, gathered 2026-11-23. Not decisions. Anything here that becomes a decision
goes to `08-decisions.md` with Dzyan's yes first (rule 5). This answers the reverb part of Q9 in
`09-open-questions.md`; Q13 (sample-rate conversion at import) also governs impulse-response files.

## The short answer

- There are two ways to make reverb. **Convolution** plays a recording of a real space (an impulse
  response) against the signal. **Algorithmic** reverb builds a tail from delay lines that feed
  back into each other. Convolution is a faithful but frozen snapshot of one room. Algorithmic is
  adjustable, alive and cheap, but is a believable room, not a particular one.
- **ReaVerb is a convolution engine.** Its "generators" also only build an impulse response that is
  then convolved. With the same impulse response and its gain options off, any correct convolver
  gives the same output to within float rounding, the same way two correct mixers sum identically
  (`04-audio-quality.md`, "The honest premise"). ReaVerb is good because convolution is exact and
  because of the impulse responses people load into it. That bar is reachable and machine-checkable.
- **Audacity's Reverb is Freeverb**, by way of SoX: eight comb filters and four allpass filters with
  fixed lengths and no movement. Section 2 shows with numbers why it rings and sounds metallic.
- A very good algorithmic reverb has: echoes that become dense quickly, enough resonances (modes)
  that none stands out, a smooth exponential decay, highs that die faster than lows, left and
  right tails that do not match, and a small amount of slow movement. Every one of these can be
  measured by a test (section 4).
- **Recommendation** (section 9): two reverb engines and one delay engine. A feedback-delay-network
  reverb with Room, Hall and Plate characters; a convolution reverb for the user's own impulse
  response files; one delay engine shown as **Delay** (clean) and **Echo** (tape). Six or seven
  controls each.
- Dzyan's `..\music-editor` reverb already uses the modern design (section 3, Signalsmith). It
  needs four fixes before it is "very good" (section 9).

## How to read this

- **(unverified)** marks a claim not confirmed in a source opened for this research.
- Every link was opened unless the entry says otherwise. Several classic papers are scanned PDFs
  that could not be read as text here; for those the entry says which summary was used.
- Licence labels used below:
  - **Port**: MIT, Apache, BSD or public domain. Code may be adapted with its notice kept.
  - **Paper**: a description, no code. Implement from the description in our own words.
  - **Read-only**: GPL, LGPL, or published with no licence at all. Read to understand; never copy.
    (Written under D-011. Since D-036 made Crucible GPLv3, GPL-compatible sources could be
    ported; none were.)
- Audacity, Ardour, JUCE, Zrythm and Tracktion were not used as code sources. Audacity's reverb
  header was read to identify its algorithm only.

---

## 1. The two families

### Convolution

- **What it is:** record how a space answers a click (its impulse response, IR), then convolve:
  every input sample triggers a scaled copy of the whole IR, and all copies are summed.
- **What it captures:** everything linear about one source position, one microphone position and
  one moment in one room: early reflections, tail, colour, stereo image.
- **What it cannot do:** change over time, react to level, or be reshaped much. A real room is not
  perfectly still: air movement and temperature change the sound's travel time, so even two sweep
  measurements a few seconds apart differ in the late tail and at high frequencies, which is why
  Farina recommends one long sweep instead of averaging many
  ([Farina 2007, section 3.5](https://angelofarina.it/Public/papers/226-AES122.pdf)). Blesser argues
  that this is why slightly time-varying algorithmic reverbs can be more realistic than fixed
  convolution; Costello agrees in principle but finds the heavy modulation of old units far beyond
  anything a real hall does ([Valhalla, Modulation in reverbs](https://valhalladsp.com/?p=1563)).
  That fixed, identical-every-time quality is what people call convolution's "frozen" sound.
- **Its quality is the quality of the IR:** measurement noise, the loudspeaker's distortion, the
  microphones and any trimming are all baked in (section 5).

### Algorithmic

- **What it is:** delay lines whose outputs are mixed and fed back, with filters inside the loop so
  the decay rate can differ by frequency. Modern designs put a diffuser (a few short mixing stages)
  in front and a feedback delay network (FDN) behind it
  ([Signalsmith, Let's Write a Reverb](https://signalsmith-audio.co.uk/writing/2021/lets-write-a-reverb/)).
- **What it captures:** the statistics of a room: density, decay per frequency band, size, width.
  Not one particular room.
- **What it can do that convolution cannot:** every parameter is live; slow modulation keeps the
  tail moving; the tail is truly exponential, with no measurement noise.
- **Its quality is the quality of the design:** too few delay lines ring, too much diffusion sounds
  metallic, too much modulation sounds like a chorus (sections 3 and 4).

### What each can and cannot do

| | Convolution | Algorithmic |
|---|---|---|
| A specific real space | Yes, one snapshot, if the IR is good | No, a believable space |
| Change over time | None. Identical on every note | Slow modulation possible; too much sounds like chorus |
| Editing | Trim, fade, filter, stretch, change the IR | Size, decay per band, pre-delay, character, all live |
| Automation | Swapping IRs needs a crossfade ([Web Audio spec](https://webaudio.github.io/web-audio-api/#ConvolverNode), note under `buffer`) | Smooth for most controls; size changes are hard (section 9) |
| CPU | Grows with IR length and channel paths (4 for true stereo) | Small and fixed, independent of decay time |
| Memory | Several megabytes per instance (section 5) | Under 2 MB |
| Latency | Zero is possible but costs CPU or a helper thread | Zero |
| End of tail | Ends where the file ends; the measurement's noise floor is part of it unless faded | Exponential forever; ends in tiny numbers (denormals, section 4) |
| Determinism across machines | FFT code paths vary by CPU (section 5) | Plain arithmetic; deterministic if coefficients are |

### When each sounds authentic

- Convolution sounds most authentic on **short, static parts of a mix**: a real room on drums, a
  real hall on an orchestral bus, a real plate or spring IR. It is weakest on long sustained
  material, where the identical, unmoving tail can be heard, and when the user wants to change size
  or decay a lot.
- Algorithmic sounds most authentic when it is **dense, smooth and modest**: low modulation, decay
  per band set like a real room (highs faster), no metallic early part. Costello's view is that the
  strength of algorithmic reverb is less imitation than plausible spaces that never existed
  ([Valhalla, Modulation in reverbs](https://valhalladsp.com/?p=1563)).

---

## 2. What ReaVerb and Audacity's Reverb actually are

### ReaVerb (Cockos)

- **A convolution reverb that builds its impulse response from a chain of modules.** Modules:
  Echo Generator, Reverb Generator, File (an IR from disk), Filter (low and high pass), Normalize,
  Reverse, and Time/Gain/Stretch. The guide warns that one module, a reverb generator or one IR
  file, is often enough ([REAPER Cockos Effects Summary Guide v2.00, section 11](https://www.reaper.fm/guides/ReaEffectsGuide.pdf), a third-party guide hosted by Cockos).
- **Echo Generator:** start position, length, spacing between echoes, random error in the spacing,
  how that error is distributed, start volume and end volume. It writes discrete echoes into the IR;
  Sound On Sound uses it to add early reflections at the start of an IR
  ([SOS ReaVerb Part 2](https://www.soundonsound.com/techniques/reaverb-part-2)).
- **Reverb Generator:** start, length (decay), volume, room size, dampening, stereo width
  ([guide, 11.4](https://www.reaper.fm/guides/ReaEffectsGuide.pdf)). Those controls match
  Freeverb's parameter set; whether it is Freeverb inside is (unverified). Either way its output is
  a fixed IR, so ReaVerb's synthetic reverb is as static as a loaded file.
- **File module:** WAV or AIFF; mix to mono, normalise, and an "attenuate impulse by -18.06 dB"
  option that is on after loading ([SOS ReaVerb Part 1](https://www.soundonsound.com/techniques/reaverb-part-1)).
  The window shows the file's peak, length and sample rate. **No IRs ship with it** (same source).
- **Trim/Gain/Stretch:** start position (cuts pre-delay or early reflections), maximum length (cuts
  the tail, or makes gated effects), gain, stretch (shrinks or lengthens the IR), pre-silence (adds
  pre-delay), width and pan ([SOS Part 1](https://www.soundonsound.com/techniques/reaverb-part-1),
  [guide 11.8](https://www.reaper.fm/guides/ReaEffectsGuide.pdf)).
- **Measuring your own IR:** "Generate test tone" writes a 4-second sine sweep from 20 Hz to 20 kHz;
  "Deconvolve" turns a recording of that sweep in a room into an IR
  ([SOS Part 2](https://www.soundonsound.com/techniques/reaverb-part-2)).
- **Performance options** ([guide 11.1](https://www.reaper.fm/guides/ReaEffectsGuide.pdf)):
  - *Max FFT*: a smaller FFT uses more CPU but spreads it more evenly; a larger one uses less CPU
    but adds latency; the guide says it does not change the sound.
  - *ZL*: zero latency, for monitoring reverb while recording.
  - *LL*: an extra thread to improve low-latency performance.
  - *Pre-reverb*: places the wet signal ahead of the dry by the given milliseconds.
- **Not found in any source opened:** whether ReaVerb resamples an IR whose rate differs from the
  project, its maximum IR length, and whether it accepts 4-channel true-stereo files. All three
  (unverified).

### Audacity's Reverb

- **Lineage:** Audacity's manual says it is based on Freeverb
  ([Audacity manual: Reverb](https://manual.audacityteam.org/man/reverb.html)). The source file's
  header says it is the libSoX reverb adapted for Audacity, licensed LGPL 2.1, with its filter
  configuration based on Freeverb by Jezar Wakefield
  ([Reverb_libSoX.h](https://doxy.audacityteam.org/_reverb__lib_so_x_8h_source.html)). SoX's own
  manual describes its reverb as the Freeverb algorithm
  ([sox man page, "reverb"](https://manpages.ubuntu.com/manpages/resolute/man1/sox.1.html)).
- **Structure** (same header): per channel, 8 parallel comb filters of 1116, 1188, 1277, 1356, 1422,
  1491, 1557 and 1617 samples, and 4 series allpass filters of 225, 341, 441 and 556 samples, all at
  44.1 kHz and scaled to other rates. Each comb has a one-pole lowpass (damping) in its feedback
  path. The allpasses use a fixed 0.5. Room size scales every delay by 0.1 to 1.0. Reverberance maps
  to one feedback gain for the combs. Freeverb's structure is described the same way, eight
  lowpass-feedback combs and four allpasses, with the right channel's delays offset by a stereo
  spread of 23 samples ([JOS, Freeverb](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html)).
- **Licence:** Freeverb itself is public domain ([JOS](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html)):
  **Port**. Audacity's and SoX's versions are LGPL inside a GPL program: **Read-only**.

### Why Audacity's Reverb sounds metallic and ringy

Each reason is measurable, and Crucible's tests in section 9 check the same things.

1. **Too few modes.** The total length of all delay lines in a feedback reverb is its number of
   resonances. Schroeder's guideline is at least 0.15 × T60 × sample rate of total delay, where T60
   is the time to decay by 60 dB ([JOS, Mode Density Requirement](https://ccrma.stanford.edu/~jos/pasp/Mode_Density_Requirement.html)).
   In seconds that is simply: **sum of delay lengths ≥ 0.15 × decay time.** Freeverb's combs total
   11,024 samples, 0.25 s, which supports decays up to about 1.7 s. At Audacity's 0% room size the
   delays shrink to a tenth, which supports about 0.17 s. Longer decays than that ring.
2. **Unequal decay between combs.** All eight combs share one feedback gain. A comb loses that gain
   once per pass, and a short comb passes more often per second, so it dies faster. The longest comb
   (1617 samples, 36.7 ms) outlasts the shortest by about 45%, so the late tail is dominated by a
   few combs repeating every 30 to 37 ms. Jot and Chaigne's whole point was to make every line decay
   at the same rate at every frequency, because modes that outlast the others sound metallic
   ([Valhalla, Best Papers](https://valhalladsp.com/2021/09/22/getting-started-with-reverb-design-part-2-the-foundations/)).
   The single damping coefficient has the same problem at high frequencies.
3. **Echo density does not build.** Parallel combs produce a fixed set of periodic echo trains. In a
   real room, and in a feedback delay network, echo density keeps growing over time; in Schroeder
   designs it does not ([Valhalla, Best Papers](https://valhalladsp.com/2021/09/22/getting-started-with-reverb-design-part-2-the-foundations/), Gerzon entry).
4. **Allpasses that are too long.** Allpass filters are only uncoloured when very short, under about
   10 ms ([JOS, Schroeder Reverberators](https://ccrma.stanford.edu/~jos/pasp/Schroeder_Reverberators.html)).
   Freeverb's two longest are 10.0 and 12.6 ms at 44.1 kHz. Short series allpasses ring on pulse-like sounds; male vocals
   are the worst case ([Valhalla, diffusion and metallic artifacts](https://valhalladsp.com/2011/01/21/reverbs-diffusion-allpass-delays-and-metallic-artifacts/)).
5. **Nothing moves.** Slow random modulation was added to commercial reverbs from the EMT 250
   onwards partly to mask the periodic artifacts of small delay memories
   ([Valhalla, Modulation in reverbs](https://valhalladsp.com/?p=1563)). Freeverb has none.

---

## 3. The algorithmic lineage and what each step fixed

| Year | Work | What it added | What it fixed | Form, licence |
|---|---|---|---|---|
| 1961 | Schroeder, "Colorless" Artificial Reverberation, JAES | The allpass delay: feedforward plus feedback around a delay | Raising echo density without the coloured response of a plain feedback delay | Paper |
| 1962 | Schroeder, Natural Sounding Artificial Reverberation, JAES 10(3) p. 219 | Parallel combs (decay) into short series allpasses (density); about 1,000 echoes per second needed | Flutter and colour of single feedback loops | Paper |
| 1971-72 | Gerzon, Synthetic Stereo Reverberation, Studio Sound | Several delays mixed by an orthogonal (energy-preserving) matrix and fed back: the feedback delay network. Filters in the loop for frequency-dependent decay | Echo density that grows over time, as in a real room; stereo spread | Magazine paper |
| 1979 | Moorer, About This Reverberation Business, CMJ 3(2) | Lowpass filters inside the comb loops; a tapped delay line for early reflections; a cheaper 2-multiply allpass | Highs decaying as slowly as lows; missing early reflections | Paper |
| 1982 | Stautner and Puckette, Designing Multi-Channel Reverberators, CMJ | A 4-line FDN with stability conditions; slow random modulation of the delay lengths | Brought FDNs to a wide audience; modulation against periodic artifacts | Paper with Music 11 code |
| 1991 | Jot and Chaigne, Digital Delay Networks for Designing Artificial Reverberators, AES 90th | A damping filter per delay line, sized so every line decays at the same rate at every frequency; a tonal correction filter; a systematic design method | Metallic modes that outlast others; independent decay time per band | Paper |
| 1992 | Gardner, A Realtime Multichannel Room Simulator | "Allpass loop" reverbs: series allpasses inside one big feedback loop; nested allpasses | Small, medium and large room examples published | Paper |
| 1997 | Dattorro, Effect Design Part 1, JAES 45(9) | A complete plate reverb, figure-of-eight tank, all lengths and coefficients, modulated allpasses, "in the style of Griesinger" | The commercial secrets made public; widely re-implemented | Paper |
| ~2000 (unverified) | Freeverb (Jezar) | 8 lowpass-feedback combs, 4 allpasses, tuned constants | A free, simple reverb | Public domain C++: **Port** |
| 2009 onwards | Costello, Valhalla DSP blog | Practice notes on diffusion, modulation, plates | | Blog, no code: **Paper** |
| 2021 | Luff, Let's Write a Reverb (Signalsmith, ADC 2021) | A multichannel diffuser (delays, shuffles, polarity flips, Hadamard mixing) feeding an FDN with a Householder matrix | Fast density without long chains of metallic allpasses | Article: **Paper**. Example code has no licence: **Read-only**. Signalsmith's DSP library is MIT: **Port** |

Sources for the table: Schroeder 1962 header and its 1,000 echoes per second requirement from the
paper itself ([PDF, partly readable](https://hajim.rochester.edu/ece/sites/zduan/teaching/ece472/reading/Schroeder_1962.pdf));
every other row from Costello's annotated list of the foundational papers
([Valhalla, Best Papers](https://valhalladsp.com/2021/09/22/getting-started-with-reverb-design-part-2-the-foundations/))
and the FDN history in [JOS](https://ccrma.stanford.edu/~jos/pasp/History_FDNs_Artificial_Reverberation.html).
The original Stautner and Puckette and Dattorro PDFs are scans that could not be read as text here
([S&P](https://www.ee.columbia.edu/~dpwe/e4896/papers/StautP82-reverb.pdf),
[Dattorro](https://ccrma.stanford.edu/~dattorro/EffectDesignPart1.pdf)).

### Notes on the steps that matter for Crucible

- **Schroeder's numbers still apply.** About 1,000 echoes per second for flutter-free reverb
  ([Schroeder 1962](https://hajim.rochester.edu/ece/sites/zduan/teaching/ece472/reading/Schroeder_1962.pdf)),
  and 10,000 or more for impulsive sounds such as drums. Julius Smith adds that the decay should be
  smooth but not perfectly smooth, modes should not be too regularly spaced, and highs should decay
  faster than lows; Moorer's ideal late reverb is exponentially decaying white noise
  ([JOS, Late Reverberation](https://ccrma.stanford.edu/~jos/Reverb/Late_Reverberation.html)).
- **The FDN decay formula** (Jot, via Smith): a delay line of M samples must lose
  60 × M / (T60 × sample rate) dB per pass, that is a gain of 0.001^(M / (T60 × fs))
  ([JOS, Achieving Desired Reverberation Times](https://ccrma.stanford.edu/~jos/pasp/Achieving_Desired_Reverberation_Times.html)).
  Make T60 a function of frequency and the gain becomes a lowpass filter per line; first-order
  filters per line are enough for good results
  ([JOS, Damping Filters](https://ccrma.stanford.edu/~jos/pasp/Damping_Filters_Reverberation_Delay.html)).
  Because shortening the decay in a band also removes energy from that band, Jot adds a small
  tonal correction filter on the output
  ([JOS, Tonal Correction Filter](https://ccrma.stanford.edu/~jos/pasp/Tonal_Correction_Filter.html)).
- **Delay lengths:** choose them mutually prime (no shared factors) and long enough for mode
  density; too few modes is heard as ringing tones or uneven amplitude modulation in the tail
  ([JOS, Choice of Delay Lengths](https://ccrma.stanford.edu/~jos/pasp/Choice_Delay_Lengths.html)).
- **Matrices:** the Householder matrix, I − (2/N)·(all ones), costs only 2N − 1 additions and needs
  no multiplies when N is a power of two; but as N grows its diagonal dominates and the network
  behaves more like separate combs ([JOS, Householder Feedback Matrix](https://ccrma.stanford.edu/~jos/pasp/Householder_Feedback_Matrix.html)).
  Signalsmith therefore puts a Hadamard diffuser (full mixing) in front for fast density, and a
  Householder matrix (lighter mixing) in the loop, arguing that heavy mixing in the loop pulls
  resonances together and leaves audible gaps in long decays. Both matrices preserve energy, so
  they do not colour the sound
  ([Signalsmith](https://signalsmith-audio.co.uk/writing/2021/lets-write-a-reverb/)).
- **Modulation:** modulating only some lines is enough, because the matrix spreads the movement
  ([Signalsmith](https://signalsmith-audio.co.uk/writing/2021/lets-write-a-reverb/)). Modulating the
  short input allpasses gives an audible chorus or "water in a metal pan" sound; modulating longer
  ones helps ([Valhalla, diffusion](https://valhalladsp.com/2011/01/21/reverbs-diffusion-allpass-delays-and-metallic-artifacts/)).
- **Diffusion is a trade-off, not a free win.** More allpass diffusion gives faster density but more
  metallic colour; fewer allpasses keep vocals clean but sound grainy at the start; very long chains
  make the attack fade in. Lexicon exposed the trade-off as the "Diffusion" control, which pushes the
  choice onto the user ([Valhalla, diffusion](https://valhalladsp.com/2011/01/21/reverbs-diffusion-allpass-delays-and-metallic-artifacts/)).
  Costello's ValhallaRoom early section uses no allpasses at all (same source).
- **Dattorro's plate** (constants from an implementation that scales them from the paper's
  29,761 Hz design rate: [MUMT618 DREV](https://kaseypocius.github.io/MUMT618-DREV/how); the paper
  scan was not readable here). Input: bandwidth lowpass, four input allpasses of 142, 107, 379 and
  277 samples. Tank, two halves cross-coupled: a modulated allpass (672 or 908), a delay (4453 or
  4217), a damping lowpass, a decay gain, a second allpass (1800 or 2656), a delay (3720 or 3163).
  Seven output taps per side. Coefficients around 0.75 and 0.625 to 0.65 for input diffusion, 0.7
  and 0.5 in the tank (implementation defaults; exact paper values unverified).
- **Rust reference:** fundsp has three stereo reverbs: a 32-channel FDN, a 32-channel "hybrid" FDN
  with diffusion and modulation, and an allpass loop
  ([fundsp docs](https://docs.rs/fundsp/latest/fundsp/prelude/fn.reverb_stereo.html)). MIT or
  Apache: **Port**.
- **zita-rev1** (Fons Adriaensen), an 8-line FDN that Smith analyses
  ([JOS FDN chapter](https://ccrma.stanford.edu/~jos/pasp/FDN_Reverberation.html)): GPL
  (unverified), **Read-only**.

---

## 4. What makes reverb sound real, as numbers

Every row is something a test can compute from an impulse response or a noise render. Targets
marked *proposed* are engineering choices for Crucible, not published standards.

| Property | Why it matters | How a test measures it | Target |
|---|---|---|---|
| Smooth exponential decay | Uneven decay is heard as flutter or pumping | Schroeder curve: backward-integrate the squared IR, in dB. A straight line means exponential decay. Linear fit from −5 to −35 dB | Fit correlation r ≤ −0.99 (REW treats worse than −0.98 as unreliable) |
| Decay time per band | The main "size" cue | Octave bands 125 Hz to 8 kHz; T30 = 2 × time from −5 to −35 dB; T20 = 3 × time from −5 to −25 dB; EDT = 6 × time to −10 dB | T30 within ±10% of the setting at 500 Hz and 1 kHz (*proposed*) |
| Highs decay faster | Walls absorb more at high frequencies; air adds to it in big spaces | T30 at 4 and 8 kHz divided by T30 at 1 kHz | Within ±15% of the Damping setting (*proposed*) |
| Early decay versus late decay | Perceived reverberance follows the early decay | EDT compared with T30 | Hall and Plate: EDT within 20% of T30 (*proposed*) |
| Echo density | Too sparse sounds grainy or fluttery | Normalised echo density (Abel and Huang): share of samples in a sliding window that lie outside one standard deviation, divided by 0.3173, the share for Gaussian noise. 1.0 means fully dense | Reaches 0.9 within 10 ms (Plate), 30 ms (Room), 60 ms (Hall) of the first wet sample, and stays there (*proposed*) |
| Mode density | Too few modes ring | Static check, no audio: sum of loop delay lengths in seconds | ≥ 0.15 × the longest decay allowed at that size (Schroeder via JOS) |
| No ringing modes | Some frequencies outlasting the rest is the metallic sound | T30 in 1/12-octave bands, 200 Hz to 8 kHz | No band more than 20% above the median of its neighbours within half an octave (*proposed*) |
| No flutter | Periodic echoes are heard as flutter | Autocorrelation of the tail's 5 ms energy envelope, exponential trend removed | Below 0.3 at every lag from 10 to 300 ms (*proposed*) |
| Early reflections, then tail | The early part carries size and position; the late part carries envelopment | Echo density profile; the point where it reaches 1.0 approximates the perceptual mixing time | Per character, see the echo density row |
| Pre-delay | Separates the dry sound from the room | Sample index of the first wet sample | Exactly round(ms × rate / 1000) plus the structure's fixed minimum |
| Stereo decorrelation | Envelopment needs different left and right tails | Correlation coefficient of left and right wet output for mono noise input, broadband and per octave from 250 Hz | Between −0.3 and +0.3; mono sum within ±1.5 dB of the summed channel energies (*proposed*) |
| Modulation stays subtle | Too much is heard as chorus, worst on piano (unverified) | Static check: pitch deviation per modulated line = 2π × rate × depth | ≤ 2 cents (*proposed*) |
| Level does not jump with decay | Changing decay should not need a gain ride | Wet RMS for pink noise across decay settings | Within ±3 dB (*proposed*; the music-editor test uses ±4 dB) |
| Same sound at 44.1, 48 and 96 kHz | One project on three machines | Same settings rendered at each rate | T30 per band within ±5%; early taps within one sample of their millisecond positions |
| Denormals | Feedback tails decay into tiny numbers that are very slow to compute | Time per block during a silent tail versus a loud block | Silent tail no slower than 1.5× a loud block; output exactly 0.0 once below −150 dBFS |

Notes on the measurements:

- **Definitions.** The Schroeder curve, EDT, T20 and T30 and the −0.98 reliability threshold are as
  documented by REW ([REW Filtered IR graph](https://www.roomeqwizard.com/help/help/html/graph_filteredir.html)).
  Masovic's lecture notes give the same factors (2 for T30, 3 for T20, 6 for EDT) and explain that
  backward integration over background noise makes the decay look slower, so the noise part must be
  cut first ([Masovic, Room Acoustics, section 6.2](https://arxiv.org/pdf/2111.01900)). REW finds the
  cut point with Lundeby's iterative method. For Crucible's own synthetic renders there is no noise,
  so the full tail can be integrated; for user IR files (section 5) the cut is needed.
- **Air absorption** matters only above about 1 kHz over long paths, around 100 m or more, so in
  large halls and churches; in ordinary rooms, absorbing surfaces dominate the faster high-frequency
  decay ([Masovic, section 3](https://arxiv.org/pdf/2111.01900)).
- **Echo density** method: Abel and Huang, AES 121st Convention paper 6985, 2006
  ([AES listing](https://www.aes.org/e-lib/browse.cfm?elib=13819); paper paywalled, method taken from
  the abstract and later citations). The window length used in the paper is (unverified); 20 ms is a
  common choice (unverified).
- **Mixing time:** a listening study found the perceived change from early reflections to diffuse
  tail can be predicted from room volume (√V) and volume-to-surface ratio, and from the echo density
  profile ([Lindau, Kosanke, Weinzierl, JAES 2012](https://www.aes.org/e-lib/browse.cfm?elib=16633),
  abstract only). Echo density in a real room grows roughly with the square of time (unverified, the
  standard image-source result).
- **Stereo:** IACC (interaural cross-correlation, the peak of the normalised left/right
  correlation within ±1 ms) is the concert-hall measure; the late part (80 ms to 1 s) goes with
  envelopment ([Masovic, section 6](https://arxiv.org/pdf/2111.01900)). Listeners are very
  sensitive to small changes when correlation is high and almost insensitive when it is already low
  ([Klockgether and van de Par, DAGA 2014](https://pub.dega-akustik.de/DAGA_2014/data/articles/000112.pdf)).
  So the target is "low enough", not "as low as possible". IACC is defined for dummy-head
  recordings; for a reverb's two output channels Crucible uses the plain correlation coefficient.
- **Sample rate:** define every delay in milliseconds. At start-up convert to samples at the actual
  rate, then move each to the nearest unused prime so the set stays mutually prime at every rate.
  Freeverb scales its 44.1 kHz lengths by rate / 44100
  ([Reverb_libSoX.h](https://doxy.audacityteam.org/_reverb__lib_so_x_8h_source.html)); Dattorro
  implementations scale from 29,761 Hz ([MUMT618 DREV](https://kaseypocius.github.io/MUMT618-DREV/how)).
- **Denormals** are covered for the whole engine in `12-known-daw-problems.md` P11: flush-to-zero
  flags on every audio thread. A reverb also benefits from stopping itself: once the input has been
  silent and the output is below −150 dBFS, clear the state and skip processing. That also serves the
  "Idle: near 0% CPU" budget in `01-vision-and-priorities.md`.

---

## 5. Convolution done well

### The arithmetic options

- **Direct convolution** has no latency but costs IR-length multiplies per sample. Far too slow for
  a reverb.
- **FFT block convolution** is fast but delays the output by a block. Splitting the IR into equal
  partitions keeps the block small. **Uniformly partitioned convolution** keeps the transformed input
  blocks in a "frequency-domain delay line", so each block needs one FFT, one inverse FFT and one
  complex multiply-add per partition. An FFT twice the block length is a good default; tuning it can
  save up to 65% for long filters at low latency
  ([Wefers and Vorländer, DAFx-11](https://www.dafx.de/paper-archive/2011/Papers/74_e.pdf)).
- **Non-uniform partitioning** (small partitions at the start of the IR, large ones later) is the
  standard method for filters longer than about 10,000 taps (same paper). Gardner's 1995 paper
  combined a direct-form head with FFT blocks for zero input-output delay
  ([JAES 43(3) pp. 127-136](https://www.aes.org/e-lib/browse.cfm?elib=7957); the AES page refused
  automated access, so this is from the abstract as listed in search results). Wefers' thesis covers
  all variants ([RWTH record 466561](https://publications.rwth-aachen.de/record/466561); not opened,
  the site served a bot check).
- **Patents:** two patents on low-latency multi-size partitioning turned up, Lake DSP's
  ([US5502747](https://patents.google.com/patent/US5502747A/en), filed 1993) and Microsoft's
  ([US6625629](https://patents.google.com/patent/US6625629B1/en), filed 2000). Google Patents lists
  both as expired (its own estimate, not a legal opinion).
- **How fft-convolver gets zero latency:** it recomputes the FFT of the partly filled current block
  on every call, so output is never late; the cost is one FFT pair per callback however few samples
  arrive (read in [lib.rs](https://github.com/neodsp/fft-convolver), MIT).
- **Average cost is not worst-case cost.** fft-convolver's two-stage version processes the whole
  tail block in the one callback where the tail buffer fills (read in `src/two_stage.rs`). That
  callback is much heavier than the others. A real-time engine is limited by its worst callback, so
  the tail work must be spread over callbacks or moved to a helper thread. ReaVerb's "LL" option is
  an extra thread for exactly this ([guide 11.1](https://www.reaper.fm/guides/ReaEffectsGuide.pdf)).
  The W3C's early convolution notes say the same: low or zero latency really needs a multi-threaded
  design ([W3C 2015 convolution note](https://www.w3.org/TR/2015/WD-webaudio-20151208/convolution.html)).

### CPU and memory

fft-convolver's own benchmark (mono, 512-sample blocks, AMD Ryzen 9900X desktop;
[README](https://github.com/neodsp/fft-convolver), [bench source](https://github.com/neodsp/fft-convolver/blob/main/benches/convolver.rs)):

| IR length (samples) | At 48 kHz | Uniform | Two-stage | Uniform, share of one core | Two-stage, share of one core |
|---|---|---|---|---|---|
| 4,096 | 0.09 s | 3.7 µs | 5.9 µs | 0.03% | 0.06% |
| 16,384 | 0.34 s | 10.9 µs | 10.0 µs | 0.10% | 0.09% |
| 65,536 | 1.37 s | 40.8 µs | 16.1 µs | 0.38% | 0.15% |
| 131,072 | 2.73 s | 81.8 µs | 18.6 µs | 0.77% | 0.17% |

- The share columns are computed here: time per block divided by 10.67 ms (512 samples at 48 kHz).
- True stereo needs four of these paths. With uniform partitions, halving the block size roughly
  doubles the cost per second (the multiply-adds per callback stay about the same while callbacks
  double). The laptop will be slower than this desktop (unverified by how much). Measure on the
  laptop before setting the budget.
- **Memory** (computed here): the IR's spectra take about twice the IR's own size per path, and the
  input history about twice the IR size per input channel. A 4-second true-stereo IR at 48 kHz is
  roughly 9 MB per instance; at 96 kHz roughly 18 MB. Three such instances at 96 kHz would use a
  third of the 150 MB app budget. One shared reverb on an aux send is the cheap pattern.

### Mono, stereo and true stereo

- **Mono IR:** one path; the same reverb on both sides unless the input is stereo.
- **Stereo IR:** left input through the left IR, right through the right. Width comes from the IR.
- **True stereo:** four channels, in the order left-to-left, left-to-right, right-to-left,
  right-to-right. A source on the left of the stage excites the room differently from one on the
  right, and true stereo keeps that. This order is used by the IR LV2 convolver
  ([linux-audio-user, 2011](https://lists.linuxaudio.org/hyperkitty/list/linux-audio-user@lists.linuxaudio.org/message/5PQQGAYMK5I3RDO3Q7HFKSNDOGIJFDYH))
  and by Steinberg's REVerence (from its manual page as quoted in search results; the page itself
  returned an error, so unverified). The Web Audio standard does matrix "true" stereo convolution
  when a 4-channel buffer meets a stereo input
  ([Web Audio, ConvolverNode](https://webaudio.github.io/web-audio-api/#ConvolverNode)). Some vendors
  ship true stereo as two stereo files instead of one 4-channel file (search results, unverified).

### Preparing an IR file (all off the audio thread)

1. **Decode** WAV or FLAC to 32-bit float.
2. **Sample rate.** The Web Audio standard refuses an IR whose rate differs from the context
   ([ConvolverNode](https://webaudio.github.io/web-audio-api/#ConvolverNode)), leaving conversion to
   the application. Crucible's rule 1 allows one conversion at the edge with a visible label. The
   choice between refusing and converting is Q13 in `09-open-questions.md`; IR files should follow
   whatever Dzyan decides there. A rate mismatch left uncorrected changes the room's size and pitch.
3. **Trim leading silence** to just before the first sample within about 60 dB of the peak, keeping
   the direct sound so the IR's timing against the dry signal stays honest. Pre-delay is then a
   separate, explicit control. ReaVerb's start position does the same job by hand
   ([SOS Part 1](https://www.soundonsound.com/techniques/reaverb-part-1)).
4. **Remove DC** (any constant offset), which otherwise becomes a slow thump.
5. **Find the noise floor and fade it out.** Real IRs end in measurement noise; the Schroeder curve
   shows a knee where decay turns into noise ([Masovic, section 6.2](https://arxiv.org/pdf/2111.01900)).
   Fade smoothly to zero from just before the knee. This also gives an honest decay time to display.
6. **Normalise for level, visibly.** The Web Audio standard scales by the IR's RMS power with a fixed
   calibration (0.00125), a correction for sample rate, and an extra factor of 0.5 for 4-channel IRs,
   so that different IRs come out at similar loudness; it can be switched off
   ([ConvolverNode, calculateNormalizationScale](https://webaudio.github.io/web-audio-api/#ConvolverNode)).
   ReaVerb offers "Normalize impulse" and a −18 dB attenuation for the same reason
   ([guide 11.3](https://www.reaper.fm/guides/ReaEffectsGuide.pdf)). Crucible: energy normalisation
   on by default, shown in the effect panel, off for the null test.
7. **Cap the length** (proposed: 10 s) and report memory use.
8. **Precompute the partitions**, then hand the finished convolver to the audio thread in one swap.
   Changing IRs while playing should crossfade between the old and new convolver
   ([Web Audio](https://webaudio.github.io/web-audio-api/#ConvolverNode), note under `buffer`).

### How IRs are measured, and why it matters

- The usual method is Farina's exponential sine sweep: play a slow sweep, record it, convolve the
  recording with the time-reversed sweep. Loudspeaker distortion lands **before** the clean impulse
  response and can be cut off; noise rejection is better than with the older MLS method; and one long
  sweep avoids the smearing that averaging causes when the air moves
  ([Farina 2007](https://angelofarina.it/Public/papers/226-AES122.pdf); the original 2000 paper is
  [AES 108th, preprint 5093](https://angelofarina.it/Public/list_pub_auro.htm)).
- Details that show in the result: fades on the sweep cause ringing before the direct sound
  (pre-ringing); low-frequency pre-ringing comes from the playback chain
  ([Farina 2007, section 3.1](https://angelofarina.it/Public/papers/226-AES122.pdf)).
- What a user hears as "a bad IR" is usually one of these: noise in the tail, distortion that was not
  separated, pre-ringing, or a mismatched sample rate. Steps 3 to 6 above repair what can be repaired.
- Measuring IRs (a sweep generator and deconvolution, as ReaVerb has) is out of scope for Crucible's
  first version. It goes to `09-open-questions.md` if Dzyan wants it.

### Determinism across machines

- RustFFT picks AVX, SSE4.1 or NEON code paths at run time from the CPU's features
  ([RustFFT README](https://github.com/ejmahler/RustFFT)). Different paths round differently, so a
  convolution reverb's output may differ in the last bits between two machines (unverified in
  practice). The difference is far below hearing but can flip the lowest bit of a 24-bit export, so
  exports would not hash the same across machines. Options: accept it and compare across machines
  with a tolerance, or build RustFFT with its `avx` and `sse` features off (slower, one code path).
- The same applies to algorithmic reverbs that compute coefficients or LFOs with `sin`, `exp` or
  `powf`: Rust documents their precision as unspecified, varying by platform and Rust version
  ([f64::sin](https://doc.rust-lang.org/std/primitive.f64.html#method.sin)). Crucible's own effects
  should compute LFOs and coefficients with their own small deterministic routines.

---

## 6. Freely usable impulse responses

Nothing was downloaded. "Bundle" means shipping the files inside Crucible's installer. (Crucible is GPLv3 since D-036;
sound files ship alongside under their own licences, so the verdicts below still hold.)

| Library | What it is | Licence as stated | Bundle in Crucible? |
|---|---|---|---|
| **OpenAIR** (University of York) | Large archive of measured spaces | Per entry. Search results show the Jack Lyons Concert Hall and York Minster pages state CC BY 4.0, and the site says most content is Creative Commons ([site](https://www.openair.hosted.york.ac.uk/), [project page](https://www.york.ac.uk/physics-engineering-technology/research/communication-technologies/projects/open-acoustic-impulse-response-library/)). The site's TLS certificate had expired on 2026-11-23, so no OpenAIR page was opened: (unverified) | CC BY entries: yes, with attribution in a NOTICE file. ShareAlike or NonCommercial entries, if any exist: no. Check each entry |
| **EchoThief** (Dr. Chris Warren) | Unusual real spaces: caves, kilns, bridges, a torpedo room ([echothief.com](http://www.echothief.com/)) | Licence text (copy on a Boston University course page): use is welcome for derivative work such as convolving sounds; for any other use, contact the author ([licence PDF](https://www.cs.bu.edu/fac/snyder/cs583/Homeworks/ImpulseResponses/EchoThiefImpulseResponseLibrary/EchoThief%20License.pdf)) | No, unless the author agrees in writing. Users may download it themselves |
| **Voxengo free IRs** | About 40 synthetic IRs made with Voxengo's discontinued Impulse Modeler, 44.1 kHz 16-bit ([page](https://www.voxengo.com/impulses/)) | Copyright Aleksey Vaneev. Royalty-free use for any purpose including commercial. Redistribution only of complete, unaltered copies with the notice, free of charge (same page) | Not single files. The whole pack unaltered is allowed but awkward. Not measured rooms |
| **Aachen Impulse Response database** (RWTH IKS) | Binaural (dummy head) IRs: meeting room, lecture room, stairway, corridor, a former church ([page](https://www.iks.rwth-aachen.de/forschung/tools-downloads/databases/aachen-impulse-response-database/)) | "Included MIT license file" (page text; the file itself not read) | Yes with the MIT notice. Speech-oriented and binaural, so better as test material than as music presets |
| **BBC Maida Vale** (York and BBC R&D) | Two studios, 3rd-order Ambisonic and binaural, 3.4 GB ([Zenodo](https://zenodo.org/records/7267562)) | CC BY 3.0 Unported (Zenodo record) | Possible with attribution, but would need decoding to stereo first. Later, if ever |
| **MIT IR Survey** (McDermott Lab) | 271 everyday-place IRs at 1.5 m ([page](https://mcdermottlab.mit.edu/Reverb/IR_Survey.html)) | No licence on the lab page; mirrors say CC BY 4.0 (search results, unverified) | Not until the licence is confirmed |
| **OpenSLR 28** | RIRs for speech research ([page](https://www.openslr.org/28/)) | Apache 2.0 | No: 16 kHz files, useless for music |

Recommendation: ship **no third-party IRs** in the first version, as ReaVerb does. The convolution
reverb loads the user's files. Tests use IRs that Crucible generates itself (exponentially decaying
noise with known decay per band, Moorer's "ideal" late reverb), which need no download and no
licence. If Dzyan later wants a few bundled rooms, pick CC BY entries from OpenAIR after checking
each page, with his yes to download (rule 7).

---

## 7. Delay and echo done well

### Three characters

| Character | What it sounds like | How it is made |
|---|---|---|
| Clean digital | Exact copies, each quieter | A delay line and a feedback gain |
| Tape echo | Repeats get darker, softer, slightly unstable in pitch | Filtering in the loop, gentle saturation, slow random "wow" and faster "flutter" modulation of the delay time. In ValhallaDelay's Tape mode the wow and flutter rates and the EQ depend on the delay time, as on physical tape units ([Valhalla, MODE control](https://valhalladsp.com/2019/04/16/valhalladelay-the-mode-control/)) |
| Bucket-brigade (BBD) analogue | Dark, noisy, slightly gritty | The chip is surrounded by lowpass filters and a compander (compressor and expander) against aliasing, distortion and hiss; each part can be modelled ([Raffel and Smith, DAFx-10](https://dafx.de/paper-archive/details/JhVfAOFXD1lAtctkMTUODg)). Driving the compander harder creates a ducking sound ([Valhalla, MODE control](https://valhalladsp.com/2019/04/16/valhalladelay-the-mode-control/)) |

### What separates a refined delay from a basic one

| Feature | Basic delay | Refined delay | Cost |
|---|---|---|---|
| Changing the time while playing | Jumps: clicks | Clean: crossfade between old and new read positions, no pitch change. Tape: glide, which bends pitch like a real tape machine | A second read head during the fade |
| Tone of the repeats | Same as the input forever | High cut and low cut inside the feedback loop, so each repeat is darker and thinner. ValhallaDelay's EQ sits in the feedback path ([controls](https://valhalladsp.com/2019/04/16/valhalladelay-the-controls/)); ReaDelay has per-tap LPF and HPF ([ReaPlugs](https://www.reaper.fm/reaplugs/)) | Two cheap filters |
| Tempo sync | Milliseconds only | Note values, dotted and triplet ([ValhallaDelay controls](https://valhalladsp.com/2019/04/16/valhalladelay-the-controls/); ReaDelay works in quarter notes too) | Needs the project tempo; tempo changes are a time change |
| Stereo | Mono or two identical sides | Ping-pong; left/right offset ("spread"); a left/right ratio. ValhallaDelay notes a 61.8% ratio gives a pseudo-reverb where echo density builds over time ([controls](https://valhalladsp.com/2019/04/16/valhalladelay-the-controls/)) | Nothing |
| Feedback limits | Can run away and clip | Clean: capped below 100%. Tape: saturation in the loop bounds the level, so 100% and above can self-oscillate without exploding. ValhallaDelay's feedback reaches 200% for that, with self-oscillation usually starting near 100% (same source) | A soft clipper |
| Modulated reads | Rounded to whole samples: zipper noise | Interpolated reads (see below) | A few multiplies |
| Diffusion | None | Allpasses in or before the loop smear each repeat; at large settings the echo becomes a reverb. ValhallaDelay's diffusion has only Amount and Size, with modulation hard-wired so it never needs adjusting ([diffusion section](https://valhalladsp.com/2019/06/13/valhalladelay-the-diffusion-section/)) | Several allpasses |
| Ducking | None | Wet level drops while the input is playing, so echoes fill gaps instead of clouding a vocal | An envelope follower |
| Drive constant | Saturation changes volume | ValhallaDelay turns the output down by half the drive boost in dB, so loudness stays about the same at any drive ([controls](https://valhalladsp.com/2019/04/16/valhalladelay-the-controls/)) | Nothing |

### Interpolation for modulated delay reads

- **Linear** is cheap and sounds very good when the signal's bandwidth is small compared with half
  the sample rate ([JOS, Delay-Line Interpolation](https://ccrma.stanford.edu/~jos/pasp/Delay_Line_Interpolation.html)).
  For full-band audio it dulls the highs by an amount that depends on the fractional position, so a
  modulated read gets a slightly varying tone (reasoning from the same source, not stated there).
- **Allpass** keeps the level flat but is recursive: it must run continuously to settle, so it is
  not suited to reads that jump around, and very small delays need care
  ([JOS, First-Order Allpass Interpolation](https://ccrma.stanford.edu/~jos/pasp/First_Order_Allpass_Interpolation.html)).
  Not for modulated reads.
- **Cubic** (4-point Lagrange or Hermite) costs a few more multiplies and is flatter in the highs
  (unverified exact figures). The proposal is cubic for every modulated read, plain integer reads
  everywhere else, and a test that measures the error (section 9) so the choice rests on numbers.

### Stability

- A feedback loop is stable when its total gain at every frequency stays below 1. With filters in
  the loop, the loop gain is feedback × filter gain, so a boosting filter must never sit in the loop.
- Clean mode: feedback capped (proposed 98%) and the loop filters are cut-only.
- Tape mode: a soft saturator in the loop bounds the level even at 100% feedback.
- Both: denormal protection as in section 4, because a repeating tail decays into tiny numbers.

---

## 8. Rust building blocks

Versions and licences from crates.io on 2026-11-23.

| Crate | Version | Licence | Use here | Notes |
|---|---|---|---|---|
| `realfft` | 3.5.1 | MIT | Real-to-complex FFT for convolution | Built on RustFFT. Real-input FFTs make partitioned convolution nearly twice as fast ([Wefers and Vorländer](https://www.dafx.de/paper-archive/2011/Papers/74_e.pdf)) |
| `rustfft` | 6.4.1 | MIT or Apache-2.0 | Underneath `realfft` | AVX, SSE4.1 and NEON chosen at run time ([README](https://github.com/ejmahler/RustFFT)); see determinism, section 5 |
| `fft-convolver` | 0.4.0 | MIT | Ready-made uniform and two-stage partitioned convolution, zero latency, no allocation in `process` | A port of HiFi-LoFi's FFTConvolver ([MIT](https://github.com/HiFi-LoFi/FFTConvolver)). Also pulls in `thiserror` and `rtsan-standalone` (Apache-2.0); the latter's build script does nothing unless `RTSAN_ENABLE` is set and does not support Windows ([rtsan-standalone-rs](https://github.com/realtime-sanitizer/rtsan-standalone-rs)), so it downloads nothing here. Tail work is not spread across callbacks (section 5) |
| `rubato` | 5.1.0 | MIT or Apache-2.0 | Converting an IR's sample rate once, at load | Already the candidate in Q13 |
| `fundsp` | 0.23.0 | MIT or Apache-2.0 | Reference only: three reverbs and filters to read and port from | Large graph library; against the lean rule as a dependency |
| `dasp` | 0.11.0 | MIT or Apache-2.0 | Not needed | Last release 2020-05-29 |
| `no_denormals` | 0.3.0 | MIT | Flush-to-zero guard on audio threads | Or ten lines of our own, see `12-known-daw-problems.md` P11 |
| `assert_no_alloc` | 1.1.2 | BSD-1-Clause | "No allocation in process" tests | Already planned in `02-architecture.md` |

Portable reference code: Freeverb (public domain), fundsp (MIT or Apache), Signalsmith DSP library
([MIT](https://github.com/Signalsmith-Audio/dsp)), HiFi-LoFi FFTConvolver and fft-convolver (MIT).
Read-only: Audacity and SoX reverb (LGPL in GPL), Signalsmith's reverb example code (no licence),
zita-rev1 (GPL, unverified), any JUCE, Ardour, Zrythm or Tracktion code.

Lean-rule view: an FDN reverb and a delay are a few hundred lines each and need no dependency.
Convolution needs `realfft`. A convolver of our own on `realfft`, structured like fft-convolver
(MIT, credited) but spreading the tail work across callbacks, is about 300 lines and avoids two
extra crates. Using fft-convolver directly is the faster start; its worst-case callback must then
be measured.

---

## 9. Recommendation for Crucible

All of this is *proposed*. It belongs to M4 ("Built-in essentials", `06-roadmap.md`) and needs
Dzyan's yes before any code (rules 4 and 5).

### What to build

| Effect | Engine | Why this one |
|---|---|---|
| **Reverb** | Diffuser plus 16-line FDN, with three characters: Room, Hall, Plate | One engine covers the common uses. It is the design Dzyan's music-editor reverb already uses; Jot's per-line damping makes it controllable per band |
| **IR Reverb** | Partitioned convolution of the user's mono, stereo or true-stereo WAV or FLAC | ReaVerb's quality bar, for real rooms, plates and springs |
| **Delay** and **Echo** | One delay engine. "Delay" opens it in Clean character, "Echo" in Tape character | Matches Dzyan's three names with two engines' worth of code |

Not proposed: a separate Dattorro plate engine, a BBD mode, shimmer, freeze, IR measurement. The
Plate character is the FDN voiced like a plate (short lines, dense diffusion, no early reflections).
If Dzyan's ears say the FDN plate is not a plate, Dattorro's tank (section 3) is the alternative,
about 150 lines.

### Reverb: controls and defaults

| Control | Range | Default | Notes |
|---|---|---|---|
| Character | Room, Hall, Plate | Hall | Changes internal constants: line lengths, early reflection pattern, diffusion, modulation |
| Size | 0 to 100% | 50% | Scales line lengths and early-reflection spacing. Not smoothly automatable: a change crossfades to a fresh tank over about 50 ms, or simply restarts the tail (decision for Dzyan) |
| Decay | 0.2 to 12 s, limited per size by the mode-density rule | Room 0.8 s, Hall 2.0 s, Plate 1.6 s | Mid-band T60, exactly as the tests measure it |
| Damping | 10 to 100%: high-frequency decay as a share of Decay | 50% | Above about 4 kHz (proposed). 100% means highs last as long as mids |
| Pre-delay | 0 to 200 ms, or a note value | Room 5 ms, Hall 20 ms, Plate 10 ms | Exact to the sample |
| Low cut | Off, 50 to 500 Hz | 120 Hz | On the wet signal only. The most-used reverb tone control in mixing (unverified as a general claim; Audacity, ReaVerbate and ReaVerb all have one) |
| Mix | 0 to 100% | 100% on an aux send, 25% as an insert | Mix 0 is exactly dry; 100% has exactly zero dry |

Not exposed: modulation rate and depth (hard-wired, as ValhallaDelay does for its diffusion),
width (always full), diffusion amount (fixed per character), early-reflection level (fixed per
character). Each can be added later if ears ask for it; each one added is a setting to get wrong.

### Reverb: internal design

- **Lengths** in milliseconds per character, converted at start-up to distinct primes at the actual
  rate. Starting points to tune by ear: Room lines 15 to 45 ms, Hall 40 to 120 ms, Plate 20 to
  60 ms, spread exponentially, all scaled by Size (0.5× to 1.5×).
- **Mode-density rule** enforced, not hoped for: Decay's maximum at each Size is
  sum of line lengths in seconds / 0.15.
- **Diffuser:** 4 steps of 16 channels (delay, shuffle, polarity flip, Hadamard), step lengths
  doubling, as Signalsmith describes; Plate uses longer steps.
- **Early reflections** (Room and Hall): 8 to 12 seeded taps within the first 10 to 80 ms (Room) or
  20 to 120 ms (Hall), alternating left and right, also fed into the tank. This is Moorer's tapped
  delay line and ReaVerb's echo generator in one.
- **Feedback matrix:** Householder (2N − 1 additions).
- **Per-line damping:** one first-order filter per line, designed so each line loses
  60 × length / T60(f) dB per pass at low and at high frequency (Jot). Plus Jot's first-order tonal
  correction on the output.
- **Modulation:** 4 of the 16 lines, depth 0.2 to 0.5 ms, rates 0.1 to 0.8 Hz, smooth random or
  sine, seeded, phase tied to the absolute sample position so renders match whatever the block size.
  Cubic interpolation on those reads only.
- **Output:** left and right from different signed sums of the lines; checked by the decorrelation
  test.
- **Level:** wet energy compensated so Decay does not change loudness (the music-editor reverb
  already scales by √(1 − g²)).
- **Real-time:** all memory allocated when the effect is created or its sample rate changes;
  coefficients computed off the audio thread and handed over; idle when silent.

### IR Reverb: controls and defaults

| Control | Range | Default | Notes |
|---|---|---|---|
| IR file | WAV or FLAC; 1, 2 or 4 channels | None | Panel shows length, channels, original rate, measured decay and any conversion |
| Normalise | On, Off | On | Energy normalisation (section 5, step 6). Off gives the file's raw level |
| Length | 10 to 100% of the IR | 100% | Shortens the tail with a smooth fade, like ReaVerb's maximum length |
| Pre-delay | 0 to 200 ms | 0 ms | Added before the IR |
| Low cut | Off, 50 to 500 Hz | Off | Wet only. Off by default so the IR is heard as recorded |
| High cut | Off, 2 to 20 kHz | Off | Wet only |
| Mix | 0 to 100% | 100% on a send, 25% as an insert | Same exact end points as Reverb |

Not exposed: stretch (if done by resampling, it shifts the room's colour along with its length),
reverse, generators. Zero
latency always; no latency setting for the user to understand.

### What to keep and fix in `..\music-editor\src\dsp\reverb.rs`

Read for this research (Dzyan's own code, MIT). It already has the diffuser, the Householder FDN,
per-line RT60 gains, seeded modulation tied to absolute position, energy compensation, and tests for
level, decay, determinism and decorrelation. Four fixes:

1. **Mode density at small sizes.** Its 8 lines span room to 2 × room with room = 20 to 150 ms, a
   total of about 11.5 × room. At Size 0 that is 0.23 s, which supports decays up to about 1.5 s,
   but Decay goes to 20 s. At Size 1 it supports about 11.5 s. Limit Decay by Size, or use 16 lines.
2. **Damping is not per line.** One one-pole lowpass with the same cutoff sits in every line, so
   short lines lose highs about twice as fast per second as long ones and the high-frequency decay
   time is not controlled. Design each line's filter from its length (Jot) and expose high-frequency
   decay as a share of Decay.
3. **Allocation and state.** `apply` allocates its delay lines on every call and starts from silence.
   A real-time version needs create (allocate) and process (no allocation, state carried between
   blocks).
4. **Determinism of `sin` and `powf`.** Rust does not guarantee their precision across platforms or
   versions ([f64::sin](https://doc.rust-lang.org/std/primitive.f64.html#method.sin)). Use our own
   LFO and coefficient routines.

`delay.rs`: integer delay times only, no smoothing when the time changes, no tempo sync, a fixed
6.5 kHz damping filter, buffers allocated per call. Keep its tests (echo lands on time, ping-pong
alternates); rebuild the rest as in section 7. `mix_gains` uses cos and sin of the mix angle, so at
100% the dry gain is 6e-17 rather than zero; set the end points exactly.

### Delay and Echo: controls and defaults

| Control | Range | Delay default | Echo default |
|---|---|---|---|
| Character | Clean, Tape | Clean | Tape |
| Time | 1 ms to 2 s, or a note value (straight, dotted, triplet) | 1/4 note | 1/8 dotted |
| Feedback | 0 to 98% (Clean), 0 to 110% (Tape, saturating) | 30% | 40% |
| Tone | High cut in the loop, 1 to 20 kHz | 12 kHz | 4.5 kHz |
| Ping-pong | Off, On | Off | Off |
| Wear (Tape only) | 0 to 100%: wow, flutter and saturation together | | 30% |
| Mix | 0 to 100% | 25% | 25% |

Low cut in the loop fixed per character (proposed 80 Hz Clean, 150 Hz Tape). Ducking and diffusion
are left out of the first version and listed in `09-open-questions.md` if Dzyan wants them.

### Machine-checkable tests

All run in `cargo test` with no audio device. "Every effect" means Reverb (each character), IR
Reverb and Delay (both characters), each at 44.1, 48 and 96 kHz.

| # | Test | Pass condition |
|---|---|---|
| 1 | Mix end points | Mix 0: output byte-identical to input. Mix 100%: zero dry. Bypassed: byte-identical |
| 2 | No allocation | `process` under `assert_no_alloc`, including parameter changes and IR swaps |
| 3 | Finite and bounded | 120 s of full-scale noise, impulses, sines and DC steps at maximum settings: every sample finite; the peak of the last 10 s is no higher than the peak of the 10 s before (no growth). Delay Clean also stays under input peak / (1 − feedback), its theoretical limit; Delay Tape under +6 dBFS |
| 4 | Denormals and idle | After input stops: output exactly 0.0 once below −150 dBFS; the effect reports idle; no tail block slower than 1.5× a loud block |
| 5 | Determinism | Two renders byte-identical; identical across host block sizes 1, 64, 128, 480 and 4,096 |
| 6 | Decay per band | Reverb impulse response: T30 within ±10% of Decay at 500 Hz and 1 kHz; 4 and 8 kHz within ±15% of Decay × Damping; Schroeder fit r ≤ −0.99 |
| 7 | Mode density | Static: for every character and size, line lengths sum to ≥ 0.15 × maximum Decay |
| 8 | No ringing modes | No 1/12-octave band from 200 Hz to 8 kHz has T30 more than 20% above its neighbours' median |
| 9 | No flutter | Tail envelope autocorrelation below 0.3 for lags 10 to 300 ms |
| 10 | Echo density | Normalised echo density ≥ 0.9 within 10 ms (Plate), 30 ms (Room), 60 ms (Hall) and until −60 dB |
| 11 | Stereo | Mono noise in: left/right correlation between −0.3 and +0.3, broadband and per octave from 250 Hz; mono sum within ±1.5 dB |
| 12 | Level | Pink-noise wet RMS within ±3 dB across Decay 0.3 to 10 s, across characters, and across IR lengths 0.5 to 6 s with normalisation on |
| 13 | Rate invariance | T30 per band within ±5% between 44.1, 48 and 96 kHz; pre-delay and early taps within one sample of their millisecond values |
| 14 | Modulation | Static: every modulated line's pitch deviation (2π × rate × depth) ≤ 2 cents |
| 15 | Smooth automation | Sweeping every automatable control over 2 s of noise: largest second difference of the output at most 2× that with fixed controls |
| 16 | Convolution correctness | Random 1 s IR and noise against a 64-bit direct convolution: error below −110 dB of peak. Unit-impulse IR: output equals input within 1e-6 |
| 17 | Zero latency | IR with one impulse at sample k: output peak at exactly k, for host buffers of 1, 32, 128 and 512 |
| 18 | Channel layouts | True stereo: each of the four paths tested alone routes only where it should. Mono and stereo IRs on mono and stereo input follow the documented rules |
| 19 | IR preparation | A generated 44.1 kHz IR in a 48 kHz project keeps its length in seconds and its T30 within ±2%, and shows the conversion label (if Q13 says convert); 500 ms of leading silence trimmed to within 1 ms; −70 dB noise appended is faded and the measured T30 moves less than 5% |
| 20 | Delay timing | Echo exactly on the sample: 120 BPM quarter note = 22,050, 24,000 and 48,000 samples |
| 21 | Delay feedback law | Clean, tone fully open: repeat k is at feedback^k within 0.05 dB |
| 22 | Delay time change | Clean: changing 250 to 400 ms during noise gives no discontinuity and no pitch change. Tape: the glide's pitch stays within a set limit |
| 23 | Interpolation quality | 1 kHz sine through a 1 ms, 1 Hz modulated read: error against the ideal frequency-modulated sine below −80 dB with cubic; linear measured for comparison |
| 24 | Tail length | Each effect's reported tail is at least the time for a 0 dBFS impulse to fall below −90 dBFS, so export renders it (`12-known-daw-problems.md` R4) |
| 25 | CPU and memory | Measured on the laptop, 48 kHz, 128-frame buffer, 60 s: Reverb ≤ 1% of one core; Delay ≤ 0.2%; IR Reverb with a 3 s true-stereo IR ≤ 2% average and worst callback ≤ 25% of the buffer period; IR Reverb memory reported per instance (proposed budgets) |

### Needs Dzyan's ears

- Does each character sound like its name? Room, Hall and Plate on male vocal (the hardest case for
  metallic colour), snare, piano and acoustic guitar.
- Is the modulation inaudible as pitch on sustained piano, yet enough to keep the tail from sounding
  frozen?
- Defaults: decay, damping, pre-delay and low cut per character; Delay and Echo tone and wear.
- Tape character: does "Wear" at 30% sound like a tape echo, and at 100% like a worn one?
- Optional objective check against the bar he named: load one IR in ReaVerb (normalise off,
  attenuate off, wet 0 dB, dry off, ZL on) and in Crucible (normalisation off), render the same dry
  file from both, and let a machine compare the two files. They should match to within float rounding.
  If they do not, one of the two is changing the IR.

### Built 2026-11-23

Dzyan asked for it to be built. The four effects exist in `crates/crucible-dsp` as proposed here,
with the choices below set to the safe option and recorded in `08-decisions.md` D-033, where the
measured CPU figures also are. Tests 1 to 7, 11, 12, 16 to 18, 20, 21 and 24 have code in
`crates/crucible-dsp/src/tests.rs` (25 runs with `--ignored` in release). Not yet coded:
8, 9 and 10 (ringing, flutter, echo density measures), 13 beyond 48 vs 96 kHz, 15 and 22
(automation does not exist yet), 19 (IR conversion: refused instead), 23.

### Decisions for Dzyan

1. Build these four effects (Reverb, IR Reverb, Delay, Echo), or fewer?
2. Size changes: crossfade to a fresh tank, or restart the tail?
3. IR files at another sample rate: follow Q13 (refuse or convert once with a label).
4. Exports with the IR Reverb may differ in the last bit between machines (section 5): accept, or
   force one FFT code path at some CPU cost?
5. Ship no impulse responses (recommended), or approve downloading a few CC BY rooms?

---

## Platform notes

- All DSP here is plain arithmetic and is the same on Windows, macOS and Linux. None of it belongs
  in a seam crate.
- Denormal handling is per CPU architecture, not per operating system: x86-64 uses the MXCSR
  flush-to-zero and denormals-are-zero flags; 64-bit ARM (Apple Silicon) uses its own control
  register (unverified for the exact register). Keep it in the one helper that
  `12-known-daw-problems.md` already asks for.
- RustFFT chooses AVX or SSE4.1 on x86-64 and NEON on 64-bit ARM at run time
  ([README](https://github.com/ejmahler/RustFFT)). Convolution output can therefore differ in the
  last bits between machines and between platforms; algorithmic effects stay bit-exact only if their
  coefficients avoid the standard library's `sin`, `exp` and `powf`.
- CPU budgets are measured on the Windows laptop, the weakest machine. They must be re-measured on
  each new platform.
- IR files are opened through the normal file path rules (relative to the project when stored in it);
  nothing platform-specific.
- No Windows-specific item was added, so the ledger in `03-platform-notes.md` is unchanged.
