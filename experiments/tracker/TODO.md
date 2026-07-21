Tracker TODO
============

Current: 135.5s of audio (HongKong_Music.it, 22 channels) rendered in
102s (0.76x real-time on fast machine). 64-channel files are ~3x slower.
(See PERF-REGRESSION.md for the 338s->120s->102s recovery: per-field
demand analysis plus redundant-force elimination.)

## Done

- [x] Prelude runtime functions seeded as concrete
- [x] If-expressions as statements in bind chains
- [x] Inline small pure functions (fi → ch * nf + field)
- [x] Flatten monadic bind chains (zero nested closures)
- [x] Typeclass methods inlined as Lua operators
- [x] Whole-program call-site analysis for parameter concreteness
- [x] UMX header detection (findIMPM)
- [x] Active channel count from IT header
- [x] Position tracking / subtrack skip-ahead on loop markers

## Performance

- [ ] Per-frame string allocations: bsConcat(bsPutI16LE(...),
  bsPutI16LE(...)) creates two 2-byte strings + one 4-byte
  concatenation per audio frame. Consider a buffer-based approach.
- [ ] readSmp is a function call per channel per frame — could be
  inlined (body is a simple if/then/else with bsIndex/bsGetI16LE).

## Tracker features

- [ ] Volume and panning slide effects (Dxy, Jxy)
- [ ] Tempo and speed change effects (Axx, Txx)
- [ ] Position jump (Bxx) and pattern break (Cxx) effects
- [ ] Vibrato, portamento, arpeggio effects
- [ ] Sample vibrato / auto-vibrato
- [ ] Global volume
- [ ] NNA (New Note Action) handling
