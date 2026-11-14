// Loaded only when a visitor asks to hear the voices. Goby's own DSP (src/dsp.rs, fx.rs,
// mic.rs and the recipes in voices.rs, in the goby.wasm the window already runs on) works
// in an AudioWorklet between a source and the visitor's headphones. The source is the
// microphone, or, given a decoded `sample`, that recording on a loop; the sample never
// touches the microphone, so it never asks for it.
export async function start(ctx, bytes, sample) {
  await ctx.audioWorklet.addModule(new URL("worklet.js", import.meta.url));
  // Raw sound: the browser's echo cancelling and noise suppression would fight the effects.
  const stream = sample ? null : await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1, latency: 0 },
  });
  try {
    // A context made outside a click can stay suspended; the page resumes it on the next one.
    await Promise.race([ctx.resume().catch(() => {}), new Promise(done => setTimeout(done, 1500))]);
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
    const input = stream ? ctx.createMediaStreamSource(stream) : ctx.createBufferSource();
    if (sample) {
      input.buffer = sample;
      input.loop = true;
    }
    // Everything reaches the speakers through this gain, so starting and stopping are ramps
    // and not clicks.
    const master = ctx.createGain();
    const rawScope = ctx.createAnalyser(), outScope = ctx.createAnalyser();
    rawScope.fftSize = outScope.fftSize = 2048;
    input.connect(node);
    node.connect(master).connect(ctx.destination);
    input.connect(rawScope);
    node.connect(outScope);
    const now = ctx.currentTime;
    master.gain.setValueAtTime(0, now);
    master.gain.linearRampToValueAtTime(1, now + 0.03);
    if (sample) input.start();
    const track = stream?.getAudioTracks()[0];
    return {
      rate: ctx.sampleRate,
      set: params => node.port.postMessage(params),
      scope(raw, out) {
        rawScope.getFloatTimeDomainData(raw);
        outScope.getFloatTimeDomainData(out);
      },
      // Milliseconds from the microphone to the ears: the browser's own buffers, as it
      // reports them, plus how far the voice runs behind. A recording has no input side.
      latency() {
        const side = track?.getSettings().latency || 0;
        const output = (ctx.baseLatency || 0) + (ctx.outputLatency || 0) + 128 / ctx.sampleRate;
        return (side + output + delay / ctx.sampleRate) * 1000;
      },
      resume() {
        if (ctx.state !== "running") ctx.resume().catch(() => {});
      },
      stop() {
        stream?.getTracks().forEach(t => t.stop());
        const t = ctx.currentTime;
        master.gain.cancelScheduledValues(t);
        master.gain.setValueAtTime(master.gain.value, t);
        master.gain.linearRampToValueAtTime(0, t + 0.04);
        setTimeout(() => ctx.close().catch(() => {}), 90);
      },
    };
  } catch (err) {
    stream?.getTracks().forEach(t => t.stop());
    throw err;
  }
}
