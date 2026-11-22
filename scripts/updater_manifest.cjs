#!/usr/bin/env node
// Writes the Tauri updater manifest (`latest.json`) for one published release.
//
// The manifest is fetched over TLS but is not itself signed; the archive's
// minisign signature (whose trusted comment carries the version, checked by
// `requireSignedVersion`) is the trust anchor. This script therefore refuses
// anything that would make the manifest disagree with the signed artifact:
// a non-semver version, an empty or unreadable signature, or a download URL
// outside this repository's release assets for that exact tag.

const fs = require('fs');
const path = require('path');

const REPOSITORY = 'jaeyoung0509/neati';
const PLATFORM = 'darwin-aarch64';
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`Error: ${message}`);
  process.exit(1);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith('--') || value === undefined || value.startsWith('--')) {
      fail(`expected --flag value pairs, got ${flag ?? '(nothing)'}`);
    }
    options[flag.slice(2)] = value;
  }
  return options;
}

function main() {
  const options = parseOptions(process.argv.slice(2));
  for (const required of ['version', 'signature', 'asset', 'output']) {
    if (!options[required]) fail(`missing --${required}`);
  }
  const { version, asset, output } = options;
  if (!SEMVER.test(version)) fail(`version is not semver: ${version}`);
  if (!/^[A-Za-z0-9._-]+\.app\.tar\.gz$/.test(asset)) fail(`asset must be a plain .app.tar.gz file name: ${asset}`);

  const signaturePath = path.resolve(options.signature);
  if (!fs.statSync(signaturePath, { throwIfNoEntry: false })?.isFile()) {
    fail(`signature file does not exist: ${signaturePath}`);
  }
  const signature = fs.readFileSync(signaturePath, 'utf8').trim();
  if (signature.length === 0) fail('signature file is empty');

  const tag = `v${version}`;
  const manifest = {
    version,
    notes: `See https://github.com/${REPOSITORY}/releases/tag/${tag}`,
    pub_date: options['pub-date'] ?? new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: {
      [PLATFORM]: {
        signature,
        url: `https://github.com/${REPOSITORY}/releases/download/${tag}/${asset}`,
      },
    },
  };
  fs.writeFileSync(path.resolve(output), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Wrote ${PLATFORM} updater manifest for ${tag} to ${output}`);
}

main();
