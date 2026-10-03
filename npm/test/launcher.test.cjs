const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

let launcher;
let loadFailure;
try {
  launcher = require('../lib/launcher.cjs');
} catch (error) {
  loadFailure = error;
}

function requireLauncher(t) {
  assert.ok(launcher, `launcher is missing: ${loadFailure?.message ?? 'unknown error'}`);
  return launcher;
}

test('maps supported operating systems to release assets', (t) => {
  const { resolveTarget } = requireLauncher(t);

  assert.deepEqual(resolveTarget('linux', 'x64'), {
    assetName: 'bornengine-linux-x86_64',
    executableName: 'bornengine',
  });
  assert.deepEqual(resolveTarget('darwin', 'arm64'), {
    assetName: 'bornengine-macos-aarch64',
    executableName: 'bornengine',
  });
  assert.deepEqual(resolveTarget('win32', 'x64'), {
    assetName: 'bornengine-windows-x86_64.exe',
    executableName: 'bornengine.exe',
  });
  assert.throws(() => resolveTarget('linux', 'arm64'), /not available/);
});

test('honors BORNENGINE_CLI_CACHE_DIR for the binary cache location', (t) => {
  const { defaultCacheDirectory } = requireLauncher(t);

  assert.equal(
    defaultCacheDirectory('linux', { BORNENGINE_CLI_CACHE_DIR: '/tmp/bornengine-cache' }, '/home/test'),
    '/tmp/bornengine-cache',
  );
});

test('downloads and verifies the matching release binary before caching it', async (t) => {
  const { ensureBinary } = requireLauncher(t);
  const cacheDir = await fs.mkdtemp(path.join(os.tmpdir(), 'bornengine-cli-cache-'));
  t.after(() => fs.rm(cacheDir, { recursive: true, force: true }));
  const binary = Buffer.from('test executable');
  const checksum = createHash('sha256').update(binary).digest('hex');
  const requested = [];
  const fetchImpl = async (url) => {
    requested.push(url);
    if (url.endsWith('/SHA256SUMS')) {
      return response(Buffer.from(`${checksum}  bornengine-linux-x86_64\n`));
    }
    if (url.endsWith('/bornengine-linux-x86_64')) return response(binary);
    throw new Error(`unexpected URL: ${url}`);
  };

  const binaryPath = await ensureBinary({
    version: '0.5.0',
    platform: 'linux',
    architecture: 'x64',
    cacheDir,
    fetchImpl,
  });

  assert.deepEqual(await fs.readFile(binaryPath), binary);
  assert.equal(requested.length, 2);
  assert.ok(requested.some((url) => url.endsWith('/SHA256SUMS')));
});

test('rejects a release binary whose checksum does not match', async (t) => {
  const { ensureBinary } = requireLauncher(t);
  const cacheDir = await fs.mkdtemp(path.join(os.tmpdir(), 'bornengine-cli-cache-'));
  t.after(() => fs.rm(cacheDir, { recursive: true, force: true }));
  const fetchImpl = async (url) =>
    url.endsWith('/SHA256SUMS')
      ? response(Buffer.from(`${'0'.repeat(64)}  bornengine-linux-x86_64\n`))
      : response(Buffer.from('wrong binary'));

  await assert.rejects(
    ensureBinary({
      version: '0.5.0',
      platform: 'linux',
      architecture: 'x64',
      cacheDir,
      fetchImpl,
    }),
    /checksum mismatch/i,
  );
  assert.deepEqual(await fs.readdir(cacheDir), []);
});

function response(body) {
  return {
    ok: true,
    status: 200,
    arrayBuffer: async () => body,
  };
}
