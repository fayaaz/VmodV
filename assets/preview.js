// WebGPU preview driver for the VmodV module catalogue.
//
// The clock, simulated audio and uniform byte layouts come from the Rust
// WebAssembly runtime (docs/assets/wasm). This file only owns GPU plumbing:
// compile the module's real WGSL, bind dummy input textures and draw a
// full-screen triangle each frame.

let wasmModulePromise;
function loadWasm() {
  if (!wasmModulePromise) {
    wasmModulePromise = import('./wasm/modv-preview.js').then(async (module) => {
      await module.default();
      return module;
    });
  }
  return wasmModulePromise;
}

function makeDummyTexture(device, size = 64) {
  const texture = device.createTexture({
    size: [size, size],
    format: 'rgba8unorm',
    usage: globalThis.GPUTextureUsage.TEXTURE_BINDING | globalThis.GPUTextureUsage.COPY_DST,
  });
  const pixels = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const offset = (y * size + x) * 4;
      pixels[offset] = (x * 255 / size) | 0;
      pixels[offset + 1] = (y * 255 / size) | 0;
      pixels[offset + 2] = ((x ^ y) & 8) ? 190 : 40;
      pixels[offset + 3] = 255;
    }
  }
  device.queue.writeTexture({ texture }, pixels, { bytesPerRow: size * 4 }, [size, size]);
  return texture.createView();
}

function makeFftTexture(device) {
  const texture = device.createTexture({
    size: [512, 1],
    format: 'rgba8unorm',
    usage: globalThis.GPUTextureUsage.TEXTURE_BINDING | globalThis.GPUTextureUsage.COPY_DST,
  });
  return texture;
}

/** Start a live preview. Returns a function that stops it and frees resources. */
export async function startPreview({ canvas, module, shader, status }) {
  if (!navigator.gpu) {
    status.textContent = 'Live previews need a browser with WebGPU (Chrome, Edge, Safari 26+).';
    return () => {};
  }

  const wasm = await loadWasm();
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    status.textContent = 'No WebGPU adapter is available on this device.';
    return () => {};
  }
  const device = await adapter.requestDevice();
  // WebGPU pipeline/draw validation is asynchronous; surface it in the panel
  // instead of silently painting nothing.
  device.addEventListener('uncapturederror', (event) => {
    status.textContent = `WebGPU error: ${event.error.message}`;
  });
  const context = canvas.getContext('webgpu');
  if (!context) {
    status.textContent = 'This browser cannot create a WebGPU canvas context.';
    return () => {};
  }
  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format, alphaMode: 'opaque' });

  const ratio = Math.min(globalThis.devicePixelRatio || 1, 2);
  const resize = () => {
    const width = Math.max(2, Math.floor(canvas.clientWidth * ratio));
    const height = Math.max(2, Math.floor(width * 9 / 16));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    engine.resize(width, height);
  };

  const engine = new wasm.PreviewEngine(canvas.width || 480, canvas.height || 270, 120);
  const sampler = device.createSampler({ magFilter: 'linear', minFilter: 'linear' });
  const dummyView = makeDummyTexture(device);
  const fftTexture = makeFftTexture(device);
  const fftData = new Uint8Array(512 * 4);

  const textureEntry = (binding) => ({
    binding,
    visibility: globalThis.GPUShaderStage.FRAGMENT,
    texture: { sampleType: 'float', viewDimension: '2d' },
  });
  const samplerEntry = (binding) => ({
    binding,
    visibility: globalThis.GPUShaderStage.FRAGMENT,
    sampler: { type: 'filtering' },
  });
  const uniformEntry = (binding) => ({
    binding,
    visibility: globalThis.GPUShaderStage.FRAGMENT,
    buffer: { type: 'uniform' },
  });

  let pipeline;
  let bindGroup;
  let uniformBuffer;
  let propBuffer = null;
  let vertexEntry = 'vs_main';
  let fragmentEntry = 'fs_main';
  let uniformBytes;

  if (module.preview.kind === 'shader') {
    const code = shader.wgsl;
    const shaderModule = device.createShaderModule({ code });
    const entries = [
      uniformEntry(0),
      ...[1, 2, 3, 4, 5, 6].map(textureEntry),
      uniformEntry(7),
      samplerEntry(8),
      samplerEntry(9),
    ];
    const layout = device.createBindGroupLayout({ entries });
    pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
      vertex: { module: shaderModule, entryPoint: vertexEntry },
      fragment: { module: shaderModule, entryPoint: fragmentEntry, targets: [{ format }] },
      primitive: { topology: 'triangle-list' },
    });
    uniformBuffer = device.createBuffer({
      size: 160,
      usage: globalThis.GPUBufferUsage.UNIFORM | globalThis.GPUBufferUsage.COPY_DST,
    });
    propBuffer = device.createBuffer({
      size: 16 * Math.max(1, module.props.length),
      usage: globalThis.GPUBufferUsage.UNIFORM | globalThis.GPUBufferUsage.COPY_DST,
    });
    const props = JSON.stringify(module.props);
    device.queue.writeBuffer(propBuffer, 0, engine.prop_bytes(props));
    uniformBytes = () => engine.builtin_uniform_bytes();
    bindGroup = device.createBindGroup({
      layout,
      entries: [
        { binding: 0, resource: { buffer: uniformBuffer } },
        ...[1, 2, 3, 4, 5].map((binding) => ({ binding, resource: dummyView })),
        { binding: 6, resource: fftTexture.createView() },
        { binding: 7, resource: { buffer: propBuffer } },
        { binding: 8, resource: sampler },
        { binding: 9, resource: sampler },
      ],
    });
  } else {
    const vertexModule = device.createShaderModule({ code: shader.vertex });
    const fragmentModule = device.createShaderModule({ code: shader.wgsl });
    const entries = [uniformEntry(0)];
    const bindEntries = [{ binding: 0, resource: {} }];
    for (const binding of shader.textureBindings) {
      entries.push(textureEntry(binding.texture), samplerEntry(binding.sampler));
      bindEntries.push(
        { binding: binding.texture, resource: dummyView },
        { binding: binding.sampler, resource: sampler },
      );
    }
    const layout = device.createBindGroupLayout({ entries });
    pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
      vertex: { module: vertexModule, entryPoint: 'vs_main' },
      fragment: { module: fragmentModule, entryPoint: 'main', targets: [{ format }] },
      primitive: { topology: 'triangle-list' },
    });
    uniformBuffer = device.createBuffer({
      size: shader.uniformLayout.size,
      usage: globalThis.GPUBufferUsage.UNIFORM | globalThis.GPUBufferUsage.COPY_DST,
    });
    bindEntries[0].resource = { buffer: uniformBuffer };
    bindGroup = device.createBindGroup({ layout, entries: bindEntries });
    const layoutJson = JSON.stringify(shader.uniformLayout);
    const propsJson = JSON.stringify(module.props);
    uniformBytes = () => engine.isf_uniform_bytes(layoutJson, propsJson);
  }

  let stopped = false;
  let frame = 0;
  let last = performance.now();

  function render(now) {
    if (stopped) return;
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    resize();

    // A small animated spectrum keeps audio-reactive modules moving.
    for (let x = 0; x < 512; x += 1) {
      const value = Math.max(0, 255 * (1 - x / 512) ** 2 * (0.45 + 0.55 * engine.bass()));
      fftData[x * 4] = value | 0;
      fftData[x * 4 + 1] = (value * 0.5) | 0;
      fftData[x * 4 + 2] = 0;
      fftData[x * 4 + 3] = 255;
    }
    device.queue.writeTexture({ texture: fftTexture }, fftData, { bytesPerRow: 512 * 4 }, [512, 1]);

    engine.tick(dt);
    let bytes;
    try {
      bytes = uniformBytes();
    } catch (error) {
      status.textContent = `Preview error: ${error}`;
      stopped = true;
      return;
    }
    device.queue.writeBuffer(uniformBuffer, 0, bytes);

    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        clearValue: { r: 0.02, g: 0.02, b: 0.05, a: 1 },
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);

    frame += 1;
    if (frame === 1 && !status.textContent.startsWith('WebGPU error')) {
      status.textContent = 'Live preview — clock and audio simulated in WebAssembly.';
    }
    requestAnimationFrame(render);
  }

  requestAnimationFrame(render);
  return () => {
    stopped = true;
    uniformBuffer.destroy();
    if (propBuffer) propBuffer.destroy();
  };
}
