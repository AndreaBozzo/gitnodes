import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dirname, '..');
const source = resolve(root, 'scripts/readme-hero.html');
const outputDir = resolve(root, 'public/screenshots');
const animatedOutput = resolve(outputDir, 'knowledge-memory.webp');
const posterOutput = resolve(outputDir, 'knowledge-memory-poster.webp');
const storyboardOutput = resolve(root, 'target/readme-hero-storyboard.png');
const width = 1200;
const height = 675;
const fps = 15;
const duration = 11.8;
const frameCount = Math.round(fps * duration);

const chromeCandidates = process.platform === 'win32'
  ? [
      `${process.env['ProgramFiles(x86)']}\\Microsoft\\Edge\\Application\\msedge.exe`,
      `${process.env.ProgramFiles}\\Microsoft\\Edge\\Application\\msedge.exe`,
      `${process.env.ProgramFiles}\\Google\\Chrome\\Application\\chrome.exe`,
    ]
  : ['/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser'];

const chrome = chromeCandidates.find(candidate => {
  try { readFileSync(candidate); return true; } catch { return false; }
});
if (!chrome) throw new Error('Chrome or Edge is required to render the README hero.');

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

async function retry(fn, attempts = 80) {
  let lastError;
  for (let i = 0; i < attempts; i++) {
    try { return await fn(); } catch (error) { lastError = error; }
    await new Promise(resolveDelay => setTimeout(resolveDelay, 100));
  }
  throw lastError;
}

class Cdp {
  constructor(url) {
    this.socket = new WebSocket(url);
    this.nextId = 1;
    this.pending = new Map();
    this.ready = new Promise((resolveReady, reject) => {
      this.socket.addEventListener('open', resolveReady, { once: true });
      this.socket.addEventListener('error', reject, { once: true });
    });
    this.socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (!message.id || !this.pending.has(message.id)) return;
      const { resolveCall, rejectCall } = this.pending.get(message.id);
      this.pending.delete(message.id);
      if (message.error) rejectCall(new Error(message.error.message));
      else resolveCall(message.result);
    });
  }

  async call(method, params = {}) {
    await this.ready;
    const id = this.nextId++;
    const result = new Promise((resolveCall, rejectCall) => {
      this.pending.set(id, { resolveCall, rejectCall });
    });
    this.socket.send(JSON.stringify({ id, method, params }));
    return result;
  }

  close() { this.socket.close(); }
}

const work = mkdtempSync(join(tmpdir(), 'gitnodes-readme-hero-'));
const frames = join(work, 'frames');
const profile = join(work, 'chrome-profile');
mkdirSync(frames);
mkdirSync(profile);
mkdirSync(outputDir, { recursive: true });
mkdirSync(dirname(storyboardOutput), { recursive: true });

const port = await freePort();
const browser = spawn(chrome, [
  '--headless=new',
  '--disable-gpu',
  '--hide-scrollbars',
  '--no-first-run',
  '--no-default-browser-check',
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${profile}`,
  `--window-size=${width},${height}`,
  'about:blank',
], { stdio: 'ignore' });

try {
  const tabs = await retry(async () => {
    const response = await fetch(`http://127.0.0.1:${port}/json`);
    if (!response.ok) throw new Error(`DevTools HTTP ${response.status}`);
    return response.json();
  });
  const page = tabs.find(tab => tab.type === 'page');
  if (!page) throw new Error('Chrome did not expose a page target.');
  const cdp = new Cdp(page.webSocketDebuggerUrl);
  await cdp.call('Page.enable');
  await cdp.call('Runtime.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride', {
    width, height, deviceScaleFactor: 1, mobile: false,
  });
  await cdp.call('Page.navigate', { url: pathToFileURL(source).href + '?t=0' });
  await retry(async () => {
    const result = await cdp.call('Runtime.evaluate', { expression: 'document.readyState' });
    if (result.result.value !== 'complete') throw new Error('Page is still loading');
  });

  for (let frame = 0; frame < frameCount; frame++) {
    const t = frame / fps;
    await cdp.call('Runtime.evaluate', { expression: `window.renderFrame(${t})` });
    const shot = await cdp.call('Page.captureScreenshot', {
      format: 'png', fromSurface: true, captureBeyondViewport: false,
    });
    writeFileSync(join(frames, `frame-${String(frame).padStart(4, '0')}.png`), Buffer.from(shot.data, 'base64'));
    if (frame % fps === 0) process.stdout.write(`Rendered ${frame}/${frameCount} frames\r`);
  }
  cdp.close();

  const ffmpeg = (args) => {
    const result = spawnSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...args], { stdio: 'inherit' });
    if (result.status !== 0) throw new Error(`ffmpeg failed with exit code ${result.status}`);
  };
  ffmpeg([
    '-framerate', String(fps), '-i', join(frames, 'frame-%04d.png'),
    '-c:v', 'libwebp_anim', '-lossless', '0', '-quality', '78', '-loop', '0', '-an',
    animatedOutput,
  ]);
  ffmpeg([
    '-i', join(frames, `frame-${String(Math.round(9.7 * fps)).padStart(4, '0')}.png`),
    '-c:v', 'libwebp', '-quality', '88',
    posterOutput,
  ]);
  const storyboardFrames = [1.0, 2.6, 4.0, 5.5, 7.7, 9.7]
    .map(seconds => `eq(n\\,${Math.round(seconds * fps)})`)
    .join('+');
  ffmpeg([
    '-framerate', String(fps), '-i', join(frames, 'frame-%04d.png'),
    '-vf', `select='${storyboardFrames}',scale=600:338,tile=2x3`,
    '-frames:v', '1', storyboardOutput,
  ]);
  process.stdout.write(`Rendered ${frameCount}/${frameCount} frames\n`);
  process.stdout.write(`Wrote ${animatedOutput}\nWrote ${posterOutput}\nWrote ${storyboardOutput}\n`);
} finally {
  browser.kill();
  await new Promise(resolveExit => {
    if (browser.exitCode !== null) resolveExit();
    else {
      browser.once('exit', resolveExit);
      setTimeout(resolveExit, 2000);
    }
  });
  const tempRoot = resolve(tmpdir());
  if (!resolve(work).startsWith(`${tempRoot}\\gitnodes-readme-hero-`) &&
      !resolve(work).startsWith(`${tempRoot}/gitnodes-readme-hero-`)) {
    throw new Error(`Refusing to remove unexpected temporary path: ${work}`);
  }
  rmSync(work, { recursive: true, force: true, maxRetries: 5, retryDelay: 250 });
}
