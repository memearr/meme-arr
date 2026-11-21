// The page's one Web Audio graph. A media element can be wired into it only once, and after that its
// sound goes out through the graph, so the wiring is remembered per element. Each wired element goes
// element -> stereo -> pan -> its own meter -> speakers, and also into one meter for the whole mix.
// The element's own volume is applied before all this, so the meters see the faders, volume lines and fades.

let context = null;
let bus = null; // {analyser, buffer}: the whole mix
let kBus = null; // {input, channels:[{analyser, buffer}, ...], sampleRate}: the whole mix, K-weighted, for loudness
const taps = new WeakMap(); // element -> {analyser, buffer, panner}
const wantedPan = new WeakMap(); // element -> pan asked for (kept for an element not wired yet)

/** The page's audio context, made on first use, or null where the browser has none. */
export function audioContext() {
  if (context) return context;
  const Ctx = typeof window !== "undefined" ? window.AudioContext || window.webkitAudioContext : null;
  if (!Ctx) return null;
  try {
    context = new Ctx();
  } catch (_) {
    context = null;
  }
  return context;
}

function meter(ctx) {
  const analyser = ctx.createAnalyser();
  analyser.fftSize = 1024;
  return { analyser, buffer: new Float32Array(analyser.fftSize) };
}

function mixBus(ctx) {
  if (!bus) {
    bus = meter(ctx);
    // nothing listens to it, but a node nobody pulls on may not be run: this pulls it, at zero volume
    const sink = ctx.createGain();
    sink.gain.value = 0;
    bus.analyser.connect(sink);
    sink.connect(ctx.destination);
  }
  return bus;
}

/**
 * The mix as a loudness meter hears it: filtered with the K-weighting of ITU-R BS.1770 (a high shelf
 * and a low cut, in the standard's shape) and split into its two channels, each with a window of the
 * last 32768 samples. Built once, the first time something is wired in; null where the browser can't.
 */
function kWeighted(ctx) {
  if (kBus) return kBus;
  try {
    const input = ctx.createGain();
    const shelf = ctx.createBiquadFilter();
    shelf.type = "highshelf";
    shelf.frequency.value = 1681.97;
    shelf.gain.value = 3.9998;
    const low = ctx.createBiquadFilter();
    low.type = "highpass";
    low.frequency.value = 38.13;
    low.Q.value = 0.5003;
    const split = ctx.createChannelSplitter(2);
    input.connect(shelf);
    shelf.connect(low);
    low.connect(split);
    const sink = ctx.createGain();
    sink.gain.value = 0;
    sink.connect(ctx.destination);
    const channels = [0, 1].map((i) => {
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 32768;
      split.connect(analyser, i);
      analyser.connect(sink); // pulled, at zero volume, so it runs
      return { analyser, buffer: new Float32Array(analyser.fftSize) };
    });
    kBus = { input, channels, sampleRate: ctx.sampleRate || 48000 };
  } catch (_) {
    kBus = null;
  }
  return kBus;
}

/** The loudness meter's windows, or null before any sound has been wired in (or without the filters). */
export function loudnessMeter() {
  return kBus;
}

/** The meter of the whole mix, or null before any sound has been wired in. */
export function mixMeter() {
  return bus;
}

/** The wiring of `el` (made the first time), or null when the browser can't. */
export function tap(el) {
  const known = taps.get(el);
  if (known) return known;
  const ctx = audioContext();
  if (!ctx) return null;
  try {
    const source = ctx.createMediaElementSource(el);
    // A mono sound is spread to both sides at full level first, as the export does, so the pan
    // law treats it as stereo and a pan of 0 leaves it as it was.
    const stereo = ctx.createGain();
    stereo.channelCount = 2;
    stereo.channelCountMode = "explicit";
    stereo.channelInterpretation = "speakers";
    const panner = ctx.createStereoPanner ? ctx.createStereoPanner() : null;
    if (panner) panner.pan.value = wantedPan.get(el) ?? 0;
    const own = meter(ctx);
    source.connect(stereo);
    const end = panner || stereo;
    if (panner) stereo.connect(panner);
    end.connect(own.analyser);
    own.analyser.connect(ctx.destination);
    end.connect(mixBus(ctx).analyser);
    const k = kWeighted(ctx);
    if (k) end.connect(k.input);
    const made = { ...own, panner };
    taps.set(el, made);
    return made;
  } catch (_) {
    return null;
  }
}

/**
 * Place `el`'s sound between the speakers: -1 left .. 0 as it is .. 1 right. Remembered for an
 * element that isn't wired in yet. Does nothing, quietly, where the browser has no panner.
 */
export function setPan(el, value) {
  if (!el) return;
  const v = Math.min(1, Math.max(-1, Number.isFinite(value) ? value : 0));
  wantedPan.set(el, v);
  const t = taps.get(el);
  if (t?.panner) t.panner.pan.value = v;
}
