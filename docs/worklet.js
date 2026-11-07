// The audio thread side of live.js: one Goby engine per node, fed 128 frames at a time.
class Goby extends AudioWorkletProcessor {
  constructor(options) {
    super();
    try {
      const module = new WebAssembly.Module(options.processorOptions.bytes);
      // The same module draws the window on the page; here its drawing imports do nothing.
      const imports = {};
      for (const i of WebAssembly.Module.imports(module)) (imports[i.module] ??= {})[i.name] = () => 0;
      this.x = new WebAssembly.Instance(module, imports).exports;
      this.at = this.x.init(sampleRate);
      this.view = new Float32Array(this.x.memory.buffer, this.at, 1024);
      this.quanta = 0;
      this.port.onmessage = e => this.set(e.data);
      this.port.postMessage({ ready: true });
    } catch (err) {
      this.port.postMessage({ error: String(err && err.message || err) });
    }
  }

  set(p) {
    this.x.set_voice(p.voice, p.tone, p.volume);
    this.x.set_mic(p.gain, p.gate, p.mix, p.enabled ? 1 : 0);
  }

  process(inputs, outputs) {
    if (!this.x) return false;
    const input = inputs[0][0], out = outputs[0], n = out[0].length;
    // A grown memory detaches the old view.
    if (this.view.buffer !== this.x.memory.buffer) this.view = new Float32Array(this.x.memory.buffer, this.at, 1024);
    if (input) this.view.set(input);
    else this.view.fill(0, 0, n);
    this.x.process(n);
    for (const channel of out) channel.set(this.view.subarray(0, n));
    if (++this.quanta % 64 === 0) this.port.postMessage({ delay: this.x.delay() });
    return true;
  }
}
registerProcessor("goby", Goby);
