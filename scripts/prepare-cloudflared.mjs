import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmod, mkdir, mkdtemp, readFile, rename, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = '2026.11.2';
// These are the digests of the signed archives attached to the GitHub release.
const downloads = new Map([
  ['x86_64-apple-darwin', {
    asset: 'cloudflared-darwin-amd64.tgz',
    sha256: '6153d9ec629174882a3adbff2f7dc5a24b4f3bdd7ccc38eaa84fba26eb890311',
  }],
  ['aarch64-apple-darwin', {
    asset: 'cloudflared-darwin-arm64.tgz',
    sha256: 'f6d58eeec91ad1ec5fd1f7e4a587303772929b2625be5d24068033851c36ff55',
  }],
]);
const outputDir = fileURLToPath(new URL('../src-tauri/binaries/cloudflared/', import.meta.url));
const target = process.argv[2] ?? process.env.TAURI_ENV_TARGET_TRIPLE
  ?? execFileSync('rustc', ['--print', 'host-tuple'], { encoding: 'utf8' }).trim();
const universal = target === 'universal-apple-darwin';
if (!universal && !downloads.has(target)) {
  throw new Error(`No macOS cloudflared binary for ${target}`);
}

async function matchesChecksum(path, sha256) {
  try {
    return createHash('sha256').update(await readFile(path)).digest('hex') === sha256;
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

async function prepareBinary(target, scratchDir) {
  const { asset, sha256 } = downloads.get(target);
  const archive = join(outputDir, `${version}-${asset}`);
  if (!await matchesChecksum(archive, sha256)) {
    const downloaded = join(scratchDir, asset);
    execFileSync('/usr/bin/curl', [
      '--fail', '--silent', '--show-error', '--location', '--retry', '2',
      '--connect-timeout', '15', '--max-time', '180',
      '--proto', '=https', '--proto-redir', '=https', '--output', downloaded,
      `https://github.com/cloudflare/cloudflared/releases/download/${version}/${asset}`,
    ], { stdio: 'inherit' });
    if (!await matchesChecksum(downloaded, sha256)) {
      throw new Error(`Checksum mismatch for cloudflared ${version} ${asset}`);
    }
    await rename(downloaded, archive);
  }

  execFileSync('/usr/bin/tar', ['-xzf', archive, '-C', scratchDir, 'cloudflared'], {
    env: { ...process.env, LC_ALL: 'C' },
  });
  const binary = join(scratchDir, 'cloudflared');
  await chmod(binary, 0o755);
  const output = join(outputDir, `cloudflared-${target}`);
  await rename(binary, output);
  return output;
}

await mkdir(outputDir, { recursive: true });
const scratchDir = await mkdtemp(join(outputDir, '.prepare-'));
try {
  if (universal) {
    const slices = [];
    for (const arch of downloads.keys()) {
      slices.push(await prepareBinary(arch, scratchDir));
    }
    const binary = join(scratchDir, 'cloudflared');
    execFileSync('/usr/bin/lipo', ['-create', ...slices, '-output', binary]);
    await chmod(binary, 0o755);
    await rename(binary, join(outputDir, `cloudflared-${target}`));
  } else {
    await prepareBinary(target, scratchDir);
  }
  console.log(`Prepared cloudflared ${version} for ${target}`);
} finally {
  await rm(scratchDir, { recursive: true, force: true });
}
