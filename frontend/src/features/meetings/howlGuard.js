//! Feedback ("howl") detector for the local mic.
//!
//! Two open mics and speakers in one physical room form a loop that echo
//! cancellation cannot break (each device only cancels its own output).
//! The loop rings at one steady, loud frequency, so this watches the mic
//! stream's spectrum for a single narrow peak that stays put and calls
//! `onHowl` once it has held long enough that speech is ruled out.

import { getHowlSettings } from "./howlSettings";

const TICK_MS = 100;
const HELD_TICKS = 5;
const FFT_SIZE = 2048;
const MIN_HZ = 150;
const MAX_HZ = 8000;
const MIN_PEAK_DB = -35; // absolute loudness floor
const MIN_PROMINENCE_DB = 17; // peak over its surrounding bins
const BIN_DRIFT = 2; // bins the peak may wander and still count as "same"
const NEIGHBOURHOOD = 10; // bins each side used as the local floor
const SKIP = 2; // bins each side of the peak left out of that floor

/// Start watching `stream`. `isActive()` is polled every tick -- while it
/// returns false (mic muted) nothing is counted. Returns a `stop` function.
export function startHowlGuard(stream, isActive, onHowl) {
  if (!stream || typeof AudioContext === "undefined") {
    console.log("howl guard: no mic stream");
    return () => {};
  }

  const context = new AudioContext();
  const analyser = context.createAnalyser();
  analyser.fftSize = FFT_SIZE;
  analyser.smoothingTimeConstant = 0;
  context.createMediaStreamSource(stream).connect(analyser);
  // Created outside a user gesture the context starts "suspended", and a
  // suspended analyser reads silence forever.
  context.resume().catch(() => {});

  const bins = new Float32Array(analyser.frequencyBinCount);
  const hzPerBin = context.sampleRate / FFT_SIZE;
  const first = Math.ceil(MIN_HZ / hzPerBin);
  const last = Math.min(Math.floor(MAX_HZ / hzPerBin), bins.length - 1);

  let heldBin = -1;
  let held = 0;
  let ticks = 0;

  const tick = () => {
    if (!getHowlSettings().enabled) {
      held = 0;
      return;
    }
    if (!isActive()) {
      held = 0;
      return;
    }
    analyser.getFloatFrequencyData(bins);

    let peak = first;
    for (let i = first; i <= last; i++) if (bins[i] > bins[peak]) peak = i;

    let floor = 0;
    let count = 0;
    for (let d = SKIP + 1; d <= NEIGHBOURHOOD; d++) {
      for (const i of [peak - d, peak + d]) {
        if (i >= 0 && i < bins.length) {
          floor += bins[i];
          count++;
        }
      }
    }
    const tonal =
      count > 0 &&
      bins[peak] > MIN_PEAK_DB &&
      bins[peak] - floor / count > MIN_PROMINENCE_DB;

    const prominenceDb = count > 0 ? bins[peak] - floor / count : 0;

    if (tonal && (held === 0 || Math.abs(peak - heldBin) <= BIN_DRIFT)) {
      held++;
      heldBin = peak;
    } else {
      held = tonal ? 1 : 0;
      heldBin = tonal ? peak : -1;
    }

    // Once a second, or on every tick while something tonal is held --
    // enough to tune the thresholds from the debug panel without a flood.
    if (tonal || ++ticks % 10 === 0) {
      console.log(
        `howl guard: ${context.state} peak ${Math.round(peak * hzPerBin)}Hz ` +
          `${bins[peak].toFixed(1)}dB (>${MIN_PEAK_DB}) ` +
          `prominence ${prominenceDb.toFixed(1)}dB (>${MIN_PROMINENCE_DB}) ` +
          `held ${held}/${HELD_TICKS}`,
      );
    }

    if (held >= HELD_TICKS) {
      held = 0;
      console.log("howl guard: FEEDBACK, muting mic");
      onHowl();
    }
  };

  const timer = setInterval(tick, TICK_MS);
  return () => {
    clearInterval(timer);
    context.close().catch(() => {});
  };
}
