// Loaded only when a visitor asks to hear the voices. Goby's own DSP (src/dsp.rs, fx.rs,
// mic.rs and the recipes in voices.rs, built from site/dsp-wasm) runs in an AudioWorklet
// between the microphone and the visitor's headphones.
export async function start(ctx) {
  const [bytes] = await Promise.all([
    fetch("goby.wasm").then(r => {
      if (!r.ok) throw new Error(`goby.wasm: ${r.status}`);
      return r.arrayBuffer();
    }),
    ctx.audioWorklet.addModule("worklet.js"),
  ]);
  // Compiled here first, so a page that may not run WebAssembly says so before it asks
  // for the microphone. The worklet compiles its own copy from the same bytes.
  await WebAssembly.compile(bytes);
  // Raw sound: the browser's echo cancelling and noise suppression would fight the effects.
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1, latency: 0 },
  });
  try {
    await ctx.resume();
    const node = new AudioWorkletNode(ctx, "goby", { outputChannelCount: [2], processorOptions: { bytes } });
    let delay = 0;
    await new Promise((ready, fail) => {
      node.port.onmessage = e => {
        if (e.data.error) fail(new Error(e.data.error));
        else if (e.data.ready) ready();
        else delay = e.data.delay;
      };
      node.onprocessorerror = () => fail(new Error("the audio worklet stopped"));
    });
    const mic = ctx.createMediaStreamSource(stream);
    const rawScope = ctx.createAnalyser(), outScope = ctx.createAnalyser();
    rawScope.fftSize = outScope.fftSize = 2048;
    mic.connect(node).connect(ctx.destination);
    mic.connect(rawScope);
    node.connect(outScope);
    const track = stream.getAudioTracks()[0];
    return {
      rate: ctx.sampleRate,
      set: params => node.port.postMessage(params),
      scope(raw, out) {
        rawScope.getFloatTimeDomainData(raw);
        outScope.getFloatTimeDomainData(out);
      },
      // What the browser says about its own buffers, plus how far the voice runs behind.
      latency() {
        const input = track.getSettings().latency || 0;
        const output = (ctx.baseLatency || 0) + (ctx.outputLatency || 0) + 128 / ctx.sampleRate;
        const voice = delay / ctx.sampleRate;
        return { input, output, voice, total: (input + output + voice) * 1000 };
      },
      stop() {
        stream.getTracks().forEach(t => t.stop());
        ctx.close();
      },
    };
  } catch (err) {
    stream.getTracks().forEach(t => t.stop());
    throw err;
  }
}
