// Mirrors minisign-verify 0.2.5 PublicKey::verify and Tauri's outer base64 encoding.
// https://docs.rs/minisign-verify/0.2.5/src/minisign_verify/lib.rs.html
// No signing key is read here; only the public key in the shipped Tauri config.
import { createHash, createPublicKey, verify } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_SIGNATURE_BYTES = 64 * 1024;
const MAX_CONFIG_BYTES = 1024 * 1024;
const ED25519_SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');
const defaultConfig = fileURLToPath(new URL('../src-tauri/tauri.conf.json', import.meta.url));

function decodeBase64(value, label) {
  if (typeof value !== 'string' || value.length === 0 || value.length > MAX_SIGNATURE_BYTES
      || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    throw new Error(`${label} is not valid base64`);
  }
  const decoded = Buffer.from(value, 'base64');
  // Buffer's decoder is permissive; reject noncanonical padding and unused bits.
  if (decoded.toString('base64') !== value) throw new Error(`${label} is not valid base64`);
  return decoded;
}

function decodeLines(value, count, label) {
  let text;
  try { text = new TextDecoder('utf-8', { fatal: true }).decode(decodeBase64(value, label)); }
  catch { throw new Error(`${label} must be base64-encoded UTF-8 minisign text`); }
  const lines = text.split(/\r?\n/);
  while (lines.at(-1) === '') lines.pop();
  if (lines.length !== count || !lines[0].startsWith('untrusted comment: ')) {
    throw new Error(`${label} is missing required minisign fields`);
  }
  return lines;
}

function decodeMetadata(publicKey, signature) {
  const publicLines = decodeLines(publicKey, 2, 'Updater public key');
  const keyPacket = decodeBase64(publicLines[1], 'Public key packet');
  if (keyPacket.length !== 42 || !['Ed', 'ED'].includes(keyPacket.subarray(0, 2).toString('latin1'))) {
    throw new Error('Updater public key has an unsupported algorithm or invalid length');
  }
  const lines = decodeLines(signature, 4, 'Updater signature');
  const signaturePacket = decodeBase64(lines[1], 'Signature packet');
  const globalSignature = decodeBase64(lines[3], 'Trusted-comment signature');
  if (signaturePacket.length !== 74 || globalSignature.length !== 64
      || !lines[2].startsWith('trusted comment: ')) {
    throw new Error('Updater signature is missing required minisign fields');
  }
  const algorithm = signaturePacket.subarray(0, 2).toString('latin1');
  if (!['Ed', 'ED'].includes(algorithm)) throw new Error('Unsupported updater signature algorithm');
  if (!keyPacket.subarray(2, 10).equals(signaturePacket.subarray(2, 10))) {
    throw new Error('Updater signature key ID does not match the configured public key');
  }
  const key = createPublicKey({
    key: Buffer.concat([ED25519_SPKI_PREFIX, keyPacket.subarray(10)]),
    format: 'der', type: 'spki',
  });
  return { key, algorithm, signature: signaturePacket.subarray(10), globalSignature, trustedComment: lines[2].slice(17) };
}

async function readText(path, limit, label) {
  const info = await stat(path);
  if (!info.isFile() || info.size > limit) throw new Error(`${label} must be a file within its size limit`);
  const bytes = await readFile(path);
  if (bytes.length > limit) throw new Error(`${label} exceeds its size limit`);
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
  catch { throw new Error(`${label} must be UTF-8`); }
}

export async function verifyUpdateSignature({ installerPath, signaturePath = `${installerPath}.sig`, configPath = defaultConfig, expectedVersion }) {
  const config = JSON.parse(await readText(configPath, MAX_CONFIG_BYTES, 'Tauri config'));
  const publicKey = config.plugins?.updater?.pubkey;
  if (typeof publicKey !== 'string' || !publicKey) throw new Error('Tauri config is missing plugins.updater.pubkey');
  const signatureText = (await readText(signaturePath, MAX_SIGNATURE_BYTES, 'Updater signature')).trim();
  const metadata = decodeMetadata(publicKey, signatureText);
  const info = await stat(installerPath);
  if (!info.isFile()) throw new Error('Installer must be a file');
  let message;
  if (metadata.algorithm === 'ED') {
    const hash = createHash('blake2b512');
    for await (const chunk of createReadStream(installerPath)) hash.update(chunk);
    message = hash.digest();
  } else {
    // Tauri allows legacy Ed signatures; only this older format needs all bytes.
    message = await readFile(installerPath);
  }
  if (!verify(null, message, metadata.key, metadata.signature)) {
    throw new Error('Installer signature verification failed');
  }
  const globalMessage = Buffer.concat([metadata.signature, Buffer.from(metadata.trustedComment, 'utf8')]);
  if (!verify(null, globalMessage, metadata.key, metadata.globalSignature)) {
    throw new Error('Trusted-comment signature verification failed');
  }
  // Match Tauri 2.12: older signatures may omit version; a signed version, when
  // present, must agree with the canonical version this repository will publish.
  const signedVersion = metadata.trustedComment.split('\t').find(field => field.startsWith('version:'))?.slice(8);
  const version = expectedVersion ?? config.version;
  if (signedVersion !== undefined && (typeof version !== 'string'
      || signedVersion.replace(/^v+/, '') !== version.replace(/^v+/, ''))) {
    throw new Error('Signed update version does not match the release version');
  }
  return { verified: true, algorithm: metadata.algorithm };
}

function parseArguments(args) {
  const names = new Map([['--installer', 'installerPath'], ['--signature', 'signaturePath'], ['--config', 'configPath'], ['--version', 'expectedVersion']]);
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const name = names.get(args[index]);
    if (!name || options[name] !== undefined || !args[index + 1] || args[index + 1].startsWith('--')) {
      throw new Error('Usage: node verify-update-signature.mjs --installer <file> [--signature <file.sig>] [--config <tauri.conf.json>] [--version <version>]');
    }
    options[name] = args[index + 1];
  }
  if (!options.installerPath) throw new Error('--installer is required');
  return options;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    await verifyUpdateSignature(parseArguments(process.argv.slice(2)));
    console.log('UPDATE_SIGNATURE_OK');
  } catch (error) {
    console.error(`Update signature verification failed: ${error.message}`);
    process.exitCode = 1;
  }
}
