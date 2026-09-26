import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { verifyUpdateSignature } from './verify-update-signature.mjs';

// Public known-answer vectors from minisign-verify 0.2.5 (lib.rs docs/tests).
// These tests never possess the private key for either official signature.
const PUBLIC_PACKET = 'RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3';
const PUBLIC_KEY = Buffer.from(`untrusted comment: minisign public key\n${PUBLIC_PACKET}\n`).toString('base64');
const OFFICIAL_ED = `untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1633700835\tfile:test\tprehashed
wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==`;
const OFFICIAL_LEGACY = `untrusted comment: signature from minisign secret key
RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=
trusted comment: timestamp:1555779966\tfile:test
QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==`;

const root = await mkdtemp(join(tmpdir(), 'agentkanban-signature-test-'));
console.log(`Signature test artifacts: ${root}`);
let sequence = 0;
const script = fileURLToPath(new URL('./verify-update-signature.mjs', import.meta.url));
const encode = text => Buffer.from(text, 'utf8').toString('base64');

async function fixture(payload = Buffer.from('test'), signature = encode(OFFICIAL_ED), publicKey = PUBLIC_KEY) {
  const prefix = join(root, String(++sequence));
  const installerPath = `${prefix}-中文 installer.exe`;
  const signaturePath = `${installerPath}.sig`;
  const configPath = `${prefix}-tauri.conf.json`;
  await writeFile(installerPath, payload);
  await writeFile(signaturePath, signature);
  await writeFile(configPath, JSON.stringify({ version: '99.2.3', plugins: { updater: { pubkey: publicKey } } }));
  return { installerPath, signaturePath, configPath };
}

function signedFixture(payload, comment = 'timestamp:1\tfile:离线测试.exe\tversion:99.2.3') {
  // An ephemeral in-memory test key: never exported, saved or printed.
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const keyId = Buffer.from('0102030405060708', 'hex');
  const rawKey = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const publicPacket = Buffer.concat([Buffer.from('Ed'), keyId, rawKey]);
  const signature = sign(null, createHash('blake2b512').update(payload).digest(), privateKey);
  const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(comment, 'utf8')]), privateKey);
  return {
    publicKey: encode(`untrusted comment: offline fixture\n${publicPacket.toString('base64')}\n`),
    signature: encode(`untrusted comment: offline fixture\n${Buffer.concat([Buffer.from('ED'), keyId, signature]).toString('base64')}\ntrusted comment: ${comment}\n${globalSignature.toString('base64')}\n`),
  };
}

test('official ED prehash and legacy Ed vectors verify', async () => {
  assert.deepEqual(await verifyUpdateSignature(await fixture()), { verified: true, algorithm: 'ED' });
  assert.deepEqual(await verifyUpdateSignature(await fixture(Buffer.from('test'), encode(OFFICIAL_LEGACY))), { verified: true, algorithm: 'Ed' });
});

test('generated ED signature verifies a streamed installer and UTF-8 trusted comment', async () => {
  const payload = Buffer.alloc(2 * 1024 * 1024, 173);
  const signed = signedFixture(payload);
  assert.equal((await verifyUpdateSignature(await fixture(payload, signed.signature, signed.publicKey))).verified, true);
});

test('tampering installer bytes is rejected', async () => {
  const files = await fixture();
  await writeFile(files.installerPath, 'Test');
  await assert.rejects(verifyUpdateSignature(files), /Installer signature verification failed/);
});

test('different key ID is rejected', async () => {
  const packet = Buffer.from(PUBLIC_PACKET, 'base64');
  packet[2] ^= 1;
  const files = await fixture(Buffer.from('test'), encode(OFFICIAL_ED), encode(`untrusted comment: public key\n${packet.toString('base64')}`));
  await assert.rejects(verifyUpdateSignature(files), /key ID does not match/);
});

test('wrong public key with the same key ID still fails cryptographic verification', async () => {
  const other = generateKeyPairSync('ed25519').publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const packet = Buffer.concat([Buffer.from(PUBLIC_PACKET, 'base64').subarray(0, 10), other]);
  const files = await fixture(Buffer.from('test'), encode(OFFICIAL_ED), encode(`untrusted comment: public key\n${packet.toString('base64')}`));
  await assert.rejects(verifyUpdateSignature(files), /Installer signature verification failed/);
});

test('tampering the payload signature is rejected', async () => {
  const lines = OFFICIAL_ED.split('\n');
  const packet = Buffer.from(lines[1], 'base64');
  packet[10] ^= 1;
  lines[1] = packet.toString('base64');
  await assert.rejects(verifyUpdateSignature(await fixture(Buffer.from('test'), encode(lines.join('\n')))), /Installer signature verification failed/);
});

test('tampering the trusted comment is rejected by the global signature', async () => {
  const signature = OFFICIAL_ED.replace('timestamp:1633700835', 'timestamp:1633700836');
  await assert.rejects(verifyUpdateSignature(await fixture(Buffer.from('test'), encode(signature))), /Trusted-comment signature verification failed/);
});

test('tampering the global signature is rejected', async () => {
  const lines = OFFICIAL_ED.split('\n');
  const packet = Buffer.from(lines[3], 'base64');
  packet[0] ^= 1;
  lines[3] = packet.toString('base64');
  await assert.rejects(verifyUpdateSignature(await fixture(Buffer.from('test'), encode(lines.join('\n')))), /Trusted-comment signature verification failed/);
});

test('a valid signature for another version cannot authorize this manifest version', async () => {
  const payload = Buffer.from('different release');
  const signed = signedFixture(payload, 'timestamp:1\tfile:app.exe\tversion:99.2.2');
  const files = await fixture(payload, signed.signature, signed.publicKey);
  await assert.rejects(verifyUpdateSignature(files), /does not match the release version/);
  assert.equal((await verifyUpdateSignature({ ...files, expectedVersion: '99.2.2' })).verified, true);
});

test('missing public key or signature file is rejected', async () => {
  const files = await fixture();
  await writeFile(files.configPath, '{}');
  await assert.rejects(verifyUpdateSignature(files), /missing plugins.updater.pubkey/);
  const other = await fixture();
  await assert.rejects(verifyUpdateSignature({ ...other, signaturePath: join(root, 'missing.sig') }), /ENOENT/);
});

test('incomplete signature, bad packet length, bad prefix and unsupported algorithm are rejected', async () => {
  const lines = OFFICIAL_ED.split('\n');
  const malformed = [
    lines.slice(0, 3),
    [lines[0], Buffer.alloc(73).toString('base64'), lines[2], lines[3]],
    [lines[0], lines[1], 'missing trusted comment prefix', lines[3]],
    [lines[0], lines[1], lines[2], Buffer.alloc(63).toString('base64')],
    [...lines, 'unexpected extra field'],
  ];
  const packet = Buffer.from(lines[1], 'base64');
  packet[0] |= 0x80;
  malformed.push([lines[0], packet.toString('base64'), lines[2], lines[3]]);
  for (const invalid of malformed) {
    await assert.rejects(verifyUpdateSignature(await fixture(Buffer.from('test'), encode(invalid.join('\n')))));
  }
});

test('invalid base64, UTF-8 and oversized signature metadata are rejected', async () => {
  for (const invalid of ['not base64!', Buffer.from([255]).toString('base64'), 'A'.repeat(65537)]) {
    await assert.rejects(verifyUpdateSignature(await fixture(Buffer.from('test'), invalid)));
  }
});

test('CLI success and failure report exit status without exposing keys', async () => {
  const files = await fixture();
  const args = [script, '--installer', files.installerPath, '--config', files.configPath];
  const success = spawnSync(process.execPath, args, { encoding: 'utf8', windowsHide: true });
  assert.equal(success.status, 0, success.stderr);
  assert.equal(success.stdout.trim(), 'UPDATE_SIGNATURE_OK');
  assert.equal(success.stderr, '');
  await writeFile(files.installerPath, 'changed');
  const failed = spawnSync(process.execPath, args, { encoding: 'utf8', windowsHide: true });
  assert.equal(failed.status, 1);
  assert.equal(failed.stdout, '');
  assert.match(failed.stderr, /Installer signature verification failed/);
  assert.ok(failed.stderr.length < 200);
  for (const extra of [['--config', files.configPath], ['--unknown', 'value']]) {
    const bad = spawnSync(process.execPath, [...args, ...extra], { encoding: 'utf8', windowsHide: true });
    assert.equal(bad.status, 1);
    assert.equal(bad.stdout, '');
  }
  assert.ok((await readFile(files.signaturePath, 'utf8')).length > 0);
});
