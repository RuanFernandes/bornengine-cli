'use strict';

const { createHash, randomUUID } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');

const RELEASE_BASE = 'https://github.com/RuanFernandes/bornengine-cli/releases/download';
const PACKAGE_VERSION = require('../package.json').version;

function resolveTarget(platform, architecture) {
  const targets = {
    'linux-x64': ['bornengine-linux-x86_64', 'bornengine'],
    'darwin-x64': ['bornengine-macos-x86_64', 'bornengine'],
    'darwin-arm64': ['bornengine-macos-aarch64', 'bornengine'],
    'win32-x64': ['bornengine-windows-x86_64.exe', 'bornengine.exe'],
  };
  const target = targets[`${platform}-${architecture}`];
  if (!target) {
    throw new Error(
      `BornEngine CLI is not available for ${platform}/${architecture}; install it with Cargo instead.`,
    );
  }
  return { assetName: target[0], executableName: target[1] };
}

function defaultCacheDirectory(platform = process.platform, env = process.env, home = os.homedir()) {
  if (env.BORNENGINE_CLI_CACHE_DIR) {
    return path.resolve(env.BORNENGINE_CLI_CACHE_DIR);
  }
  if (platform === 'win32') {
    return path.join(env.LOCALAPPDATA || path.join(home, 'AppData', 'Local'), 'BornEngine', 'cli');
  }
  if (platform === 'darwin') {
    return path.join(home, 'Library', 'Caches', 'BornEngine', 'cli');
  }
  return path.join(env.XDG_CACHE_HOME || path.join(home, '.cache'), 'BornEngine', 'cli');
}

async function ensureBinary({
  version = PACKAGE_VERSION,
  platform = process.platform,
  architecture = process.arch,
  cacheDir = defaultCacheDirectory(platform),
  fetchImpl = globalThis.fetch,
  releaseBase = RELEASE_BASE,
} = {}) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`Invalid BornEngine CLI release version: ${version}`);
  }
  if (typeof fetchImpl !== 'function') {
    throw new Error('Node.js fetch is unavailable; use Node.js 18 or newer.');
  }

  const { assetName, executableName } = resolveTarget(platform, architecture);
  const versionDirectory = path.join(cacheDir, `v${version}`);
  const binaryPath = path.join(versionDirectory, executableName);
  try {
    const existing = await fs.stat(binaryPath);
    if (existing.isFile()) return binaryPath;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }

  const releaseUrl = `${releaseBase}/v${version}`;
  const [checksumManifest, binary] = await Promise.all([
    fetchAsset(`${releaseUrl}/SHA256SUMS`, fetchImpl),
    fetchAsset(`${releaseUrl}/${assetName}`, fetchImpl),
  ]);
  const expected = checksumFor(checksumManifest.toString('utf8'), assetName);
  const actual = createHash('sha256').update(binary).digest('hex');
  if (actual !== expected) {
    throw new Error(`Checksum mismatch for ${assetName}; the CLI binary was not installed.`);
  }

  await fs.mkdir(versionDirectory, { recursive: true });
  const temporaryPath = `${binaryPath}.${process.pid}.${randomUUID()}.tmp`;
  try {
    await fs.writeFile(temporaryPath, binary, { flag: 'wx', mode: 0o755 });
    if (platform !== 'win32') await fs.chmod(temporaryPath, 0o755);
    await fs.rename(temporaryPath, binaryPath);
  } catch (error) {
    await fs.rm(temporaryPath, { force: true });
    if (error.code === 'EEXIST' || error.code === 'EPERM') {
      const existing = await fs.stat(binaryPath).catch(() => null);
      if (existing?.isFile()) return binaryPath;
    }
    throw error;
  }
  return binaryPath;
}

async function fetchAsset(url, fetchImpl) {
  let response;
  try {
    response = await fetchImpl(url, { signal: AbortSignal.timeout(30_000) });
  } catch (error) {
    throw new Error(`Could not download ${url}: ${error.message}`, { cause: error });
  }
  if (!response.ok) {
    throw new Error(`Could not download ${url}: HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

function checksumFor(manifest, assetName) {
  for (const line of manifest.split(/\r?\n/)) {
    const match = line.trim().match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (match && match[2] === assetName) return match[1].toLowerCase();
  }
  throw new Error(`SHA256SUMS does not contain a checksum for ${assetName}.`);
}

module.exports = {
  PACKAGE_VERSION,
  checksumFor,
  defaultCacheDirectory,
  ensureBinary,
  resolveTarget,
};
