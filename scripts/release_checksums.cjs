#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

function fail(message) {
  console.error(`Error: ${message}`);
  process.exit(1);
}

function parseOptions(argv) {
  const options = {};
  const positional = [];

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith('--')) {
      positional.push(argument);
      continue;
    }

    const value = argv[index + 1];
    if (value === undefined || value.startsWith('--')) {
      fail(`missing value for ${argument}`);
    }
    options[argument.slice(2)] = value;
    index += 1;
  }

  return { options, positional };
}

function requireFile(filePath, label) {
  const resolvedPath = path.resolve(filePath);
  if (!fs.statSync(resolvedPath, { throwIfNoEntry: false })?.isFile()) {
    fail(`${label} does not exist: ${resolvedPath}`);
  }
  return resolvedPath;
}

function normalizeManifest(contents, sourcePath) {
  const normalized = contents.replace(/\r\n?/g, '\n').trimEnd();
  if (!normalized) {
    fail(`checksum manifest is empty: ${sourcePath}`);
  }

  for (const line of normalized.split('\n')) {
    if (!/^[0-9a-fA-F]{64}  [^\r\n]+$/.test(line)) {
      fail(`invalid checksum entry in ${sourcePath}: ${line}`);
    }
  }

  return `${normalized}\n`;
}

function checksumFile(filePath) {
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function writeChecksum(options, positional) {
  if (positional.length > 0 || !options.file || !options.output) {
    fail('write expects --file and --output arguments');
  }

  const filePath = requireFile(options.file, 'release artifact');
  const outputPath = path.resolve(options.output);
  const hash = checksumFile(filePath);
  fs.writeFileSync(outputPath, `${hash}  ${path.basename(filePath)}\n`, 'ascii');
}

function writeChecksums(options, positional) {
  if (!options.output || positional.length === 0) {
    fail('write-many expects --output followed by one or more files');
  }

  const outputPath = path.resolve(options.output);
  const lines = positional.map((artifactPath) => {
    const filePath = requireFile(artifactPath, 'release artifact');
    const hash = checksumFile(filePath);
    return `${hash}  ${path.basename(filePath)}`;
  });

  fs.writeFileSync(outputPath, `${lines.join('\n')}\n`, 'ascii');
}

function combineChecksums(options, positional) {
  if (!options.output || positional.length === 0) {
    fail('combine expects --output followed by one or more checksum manifests');
  }

  const outputPath = path.resolve(options.output);
  const combined = positional
    .map((manifestPath) => {
      const sourcePath = requireFile(manifestPath, 'checksum manifest');
      return normalizeManifest(fs.readFileSync(sourcePath, 'utf8'), sourcePath);
    })
    .join('');
  fs.writeFileSync(outputPath, combined, 'ascii');
}

// Artifact upload success does not prove that every staged installer was
// included. Verify a downloaded directory against the original manifest and an
// explicit filename set, refusing missing, renamed, extra, or changed files.
function verifyDirectory(options, positional) {
  if (!options.manifest || !options.directory || positional.length === 0) {
    fail('verify-directory expects --manifest, --directory, and expected filenames');
  }

  const expectedNames = new Set(positional);
  if (expectedNames.size !== positional.length) {
    fail('expected artifact filenames must be unique');
  }
  for (const name of expectedNames) {
    if (!name || name === '.' || name === '..' || /[/\\\r\n]/.test(name)) {
      fail(`expected artifact filename must be a basename: ${name}`);
    }
  }

  const manifestPath = requireFile(options.manifest, 'checksum manifest');
  const manifest = normalizeManifest(fs.readFileSync(manifestPath, 'utf8'), manifestPath);
  const checksums = new Map();
  for (const line of manifest.trimEnd().split('\n')) {
    const name = line.slice(66);
    if (!expectedNames.has(name) || checksums.has(name)) {
      fail(`checksum manifest has an unexpected or duplicate artifact: ${name}`);
    }
    checksums.set(name, line.slice(0, 64).toLowerCase());
  }
  if (checksums.size !== expectedNames.size) {
    fail('checksum manifest does not cover every expected artifact');
  }

  const directoryPath = path.resolve(options.directory);
  if (!fs.lstatSync(directoryPath, { throwIfNoEntry: false })?.isDirectory()) {
    fail(`artifact directory does not exist: ${directoryPath}`);
  }
  const actualNames = fs.readdirSync(directoryPath).sort();
  const sortedExpected = [...expectedNames].sort();
  if (
    actualNames.length !== sortedExpected.length ||
    actualNames.some((name, index) => name !== sortedExpected[index])
  ) {
    fail(`artifact directory must contain exactly: ${sortedExpected.join(', ')}`);
  }

  for (const name of sortedExpected) {
    const artifactPath = path.join(directoryPath, name);
    const metadata = fs.lstatSync(artifactPath);
    if (!metadata.isFile() || metadata.size === 0) {
      fail(`artifact must be a nonempty regular file: ${name}`);
    }
    const hash = checksumFile(artifactPath);
    if (hash !== checksums.get(name)) {
      fail(`artifact checksum does not match staged bytes: ${name}`);
    }
    console.log(`${hash}  ${name}`);
  }
  console.log(`Verified ${sortedExpected.length} artifacts with exact filenames and SHA-256 contents.`);
}

const command = process.argv[2];
const { options, positional } = parseOptions(process.argv.slice(3));

if (command === 'write') {
  writeChecksum(options, positional);
} else if (command === 'write-many') {
  writeChecksums(options, positional);
} else if (command === 'combine') {
  combineChecksums(options, positional);
} else if (command === 'verify-directory') {
  verifyDirectory(options, positional);
} else {
  fail('expected write, write-many, combine, or verify-directory command');
}
