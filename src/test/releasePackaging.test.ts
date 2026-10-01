import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

const repositoryRoot = resolve(import.meta.dirname, '../..');
const temporaryDirectories: string[] = [];
const windowsInstallerNames = [
  'neati-windows-x64-setup.exe',
  'neati-windows-x64-setup-machine.exe',
];

function windowsArtifactFixture() {
  const fixtureRoot = mkdtempSync(join(tmpdir(), 'neati-windows-smoke-artifacts-'));
  temporaryDirectories.push(fixtureRoot);
  const staged = join(fixtureRoot, 'staged');
  const downloaded = join(fixtureRoot, 'downloaded');
  const manifest = join(fixtureRoot, 'SHA256SUMS.txt');
  mkdirSync(staged);
  mkdirSync(downloaded);
  for (const name of windowsInstallerNames) {
    // These are disposable byte fixtures, not executable installers.
    const contents = `staged bytes for ${name}`;
    writeFileSync(join(staged, name), contents);
    writeFileSync(join(downloaded, name), contents);
  }
  execFileSync(
    process.execPath,
    [
      'scripts/release_checksums.cjs', 'write-many', '--output', manifest,
      ...windowsInstallerNames.map((name) => join(staged, name)),
    ],
    { cwd: repositoryRoot, stdio: 'pipe' },
  );
  const verify = (directory = downloaded, expectedNames = windowsInstallerNames) =>
    execFileSync(
      process.execPath,
      [
        'scripts/release_checksums.cjs', 'verify-directory',
        '--manifest', manifest, '--directory', directory, ...expectedNames,
      ],
      { cwd: repositoryRoot, stdio: 'pipe', encoding: 'utf8' },
    );
  return { staged, downloaded, manifest, verify };
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe('release packaging contracts', () => {
  it('keeps every release manifest synchronized', () => {
    const packageVersion = JSON.parse(
      readFileSync(join(repositoryRoot, 'package.json'), 'utf8'),
    ).version;
    expect(() =>
      execFileSync(
        process.execPath,
        ['scripts/bump_version.cjs', 'check', `v${packageVersion}`],
        {
          cwd: repositoryRoot,
          stdio: 'pipe',
        },
      ),
    ).not.toThrow();
  });

  it('generates a versioned WinGet multi-file manifest for the immutable installer URL', () => {
    const fixtureRoot = mkdtempSync(join(tmpdir(), 'neati-winget-'));
    temporaryDirectories.push(fixtureRoot);
    const installer = join(fixtureRoot, 'neati-windows-x64-setup.exe');
    const installerBytes = Buffer.from('deterministic NSIS fixture');
    writeFileSync(installer, installerBytes);

    execFileSync(
      process.execPath,
      [
        'scripts/generate_winget_manifest.cjs',
        '--version',
        '0.2.0',
        '--installer',
        installer,
        '--output',
        fixtureRoot,
      ],
      { cwd: repositoryRoot, stdio: 'pipe' },
    );

    const manifestRoot = join(
      fixtureRoot,
      'manifests',
      'j',
      'jaeyoung0509',
      'Neati',
      '0.2.0',
    );
    // Reading a wrong-case path can succeed on macOS. Compare directory entry
    // spellings so this contract is equally strict on every filesystem.
    expect(readdirSync(dirname(dirname(manifestRoot)))).toEqual([
      basename(dirname(manifestRoot)),
    ]);
    const versionManifest = readFileSync(join(manifestRoot, 'jaeyoung0509.Neati.yaml'), 'utf8');
    const installerManifest = readFileSync(
      join(manifestRoot, 'jaeyoung0509.Neati.installer.yaml'),
      'utf8',
    );
    const localeManifest = readFileSync(
      join(manifestRoot, 'jaeyoung0509.Neati.locale.en-US.yaml'),
      'utf8',
    );
    const expectedHash = createHash('sha256').update(installerBytes).digest('hex').toUpperCase();

    expect(versionManifest).toContain('ManifestType: version');
    expect(installerManifest).toContain('InstallerType: nullsoft');
    expect(installerManifest).toContain('Scope: user');
    expect(installerManifest).toContain('MinimumOSVersion: 10.0.17763.0');
    expect(installerManifest).toContain('ElevationRequirement: elevationProhibited');
    expect(installerManifest).toContain(
      'InstallerUrl: https://github.com/jaeyoung0509/neati/releases/download/v0.2.0/neati-windows-x64-setup.exe',
    );
    expect(installerManifest).toContain(`InstallerSha256: ${expectedHash}`);
    expect(localeManifest).toContain('License: MIT');
    expect(versionManifest).toContain('PackageIdentifier: jaeyoung0509.Neati');
    expect(localeManifest).toContain('PackageName: neati\n');
    expect(installerManifest).toContain('DisplayName: neati\n');
    expect(localeManifest).toContain('ManifestType: defaultLocale');
  });

  it('writes and combines portable LF-only checksum manifests', () => {
    const fixtureRoot = mkdtempSync(join(tmpdir(), 'neati-checksums-'));
    temporaryDirectories.push(fixtureRoot);
    const macArtifact = join(fixtureRoot, 'neati-macos-arm64.dmg');
    const windowsArtifact = join(fixtureRoot, 'neati-windows-x64-setup.exe');
    const macManifest = join(fixtureRoot, 'SHA256SUMS-macos-arm64.txt');
    const windowsManifest = join(fixtureRoot, 'SHA256SUMS-windows-x64.txt');
    const combinedManifest = join(fixtureRoot, 'SHA256SUMS.txt');
    writeFileSync(macArtifact, 'mac fixture');
    writeFileSync(windowsArtifact, 'windows fixture');

    for (const [artifact, manifest] of [
      [macArtifact, macManifest],
      [windowsArtifact, windowsManifest],
    ]) {
      execFileSync(
        process.execPath,
        ['scripts/release_checksums.cjs', 'write', '--file', artifact, '--output', manifest],
        { cwd: repositoryRoot, stdio: 'pipe' },
      );
    }

    const windowsChecksum = readFileSync(windowsManifest, 'utf8');
    writeFileSync(windowsManifest, windowsChecksum.replace(/\n/g, '\r\n'));
    execFileSync(
      process.execPath,
      [
        'scripts/release_checksums.cjs',
        'combine',
        '--output',
        combinedManifest,
        macManifest,
        windowsManifest,
      ],
      { cwd: repositoryRoot, stdio: 'pipe' },
    );

    const combined = readFileSync(combinedManifest, 'utf8');
    expect(combined).not.toContain('\r');
    expect(combined).toMatch(/neati-macos-arm64\.dmg\n/);
    expect(combined).toMatch(/neati-windows-x64-setup\.exe\n$/);
  });

  it('verifies the exact two Windows installer filenames and unchanged uploaded bytes', () => {
    const fixture = windowsArtifactFixture();
    expect(fixture.verify(fixture.staged)).toContain('Verified 2 artifacts');
    expect(fixture.verify()).toContain('Verified 2 artifacts');
  });

  it('rejects an uploaded artifact that omits the machine-wide installer', () => {
    const fixture = windowsArtifactFixture();
    rmSync(join(fixture.downloaded, windowsInstallerNames[1]));
    expect(fixture.verify).toThrow(/artifact directory must contain exactly/);
  });

  it('rejects renamed or extra downloaded installer files', () => {
    const fixture = windowsArtifactFixture();
    renameSync(
      join(fixture.downloaded, windowsInstallerNames[1]),
      join(fixture.downloaded, 'machine-installer.exe'),
    );
    expect(fixture.verify).toThrow(/artifact directory must contain exactly/);
    renameSync(
      join(fixture.downloaded, 'machine-installer.exe'),
      join(fixture.downloaded, windowsInstallerNames[1]),
    );
    writeFileSync(join(fixture.downloaded, 'unexpected.exe'), 'unexpected bytes');
    expect(fixture.verify).toThrow(/artifact directory must contain exactly/);
  });

  it('rejects changed or empty downloaded installer bytes', () => {
    const fixture = windowsArtifactFixture();
    writeFileSync(join(fixture.downloaded, windowsInstallerNames[1]), 'changed bytes');
    expect(fixture.verify).toThrow(/artifact checksum does not match staged bytes/);
    writeFileSync(join(fixture.downloaded, windowsInstallerNames[1]), '');
    expect(fixture.verify).toThrow(/artifact must be a nonempty regular file/);
  });

  it('requires checksum coverage for every installer and refuses duplicate entries', () => {
    const fixture = windowsArtifactFixture();
    const lines = readFileSync(fixture.manifest, 'utf8').trimEnd().split('\n');
    writeFileSync(fixture.manifest, `${lines[0]}\n`);
    expect(fixture.verify).toThrow(/checksum manifest does not cover every expected artifact/);
    writeFileSync(fixture.manifest, `${lines[0]}\n${lines[0]}\n`);
    expect(fixture.verify).toThrow(/unexpected or duplicate artifact/);
  });

  it('refuses directories posing as installers and non-basename artifact requests', () => {
    const fixture = windowsArtifactFixture();
    rmSync(join(fixture.downloaded, windowsInstallerNames[1]));
    mkdirSync(join(fixture.downloaded, windowsInstallerNames[1]));
    expect(fixture.verify).toThrow(/artifact must be a nonempty regular file/);
    expect(() => fixture.verify(fixture.downloaded, ['../outside.exe']))
      .toThrow(/expected artifact filename must be a basename/);
  });

  it('uploads both explicit Windows installer paths and verifies the downloaded artifact', () => {
    const workflow = readFileSync(join(repositoryRoot, '.github/workflows/ci.yml'), 'utf8');
    const uploadStep = workflow.split('      - name: Upload Windows packaging smoke artifacts\n')[1]
      ?.split('\n      - name:')[0];
    const pathBlock = uploadStep?.match(/          path: \|\n((?:            .+\n)+)/)?.[1];
    expect(pathBlock?.trim().split('\n').map((line) => line.trim()))
      .toEqual(windowsInstallerNames.map((name) => `packaging/${name}`));
    expect(workflow).toContain('      - name: Download Windows packaging smoke artifacts for verification');
    expect(workflow).toContain('--manifest packaging-checksums.txt --directory packaging-downloaded');
  });

  it('binds endpoint review records to one exact, unique artifact set', () => {
    const fixtureRoot = mkdtempSync(join(tmpdir(), 'neati-endpoint-review-'));
    temporaryDirectories.push(fixtureRoot);
    const macArtifact = join(fixtureRoot, 'neati-macos-arm64.dmg');
    const windowsArtifact = join(fixtureRoot, 'neati-windows-x64-setup.exe');
    const reviewPath = join(fixtureRoot, 'endpoint-review.json');
    writeFileSync(macArtifact, 'reviewed mac bytes');
    writeFileSync(windowsArtifact, 'reviewed windows bytes');

    const common = [
      '--version',
      '0.3.19',
      '--commit',
      '0123456789abcdef',
    ];
    execFileSync(
      process.execPath,
      [
        'scripts/endpoint_review.cjs',
        'record',
        '--output',
        reviewPath,
        ...common,
        '--status',
        'clear',
        '--reference',
        'submission-123',
        '--reviewer',
        'release-maintainer',
        '--reviewed-at',
        '2026-09-12',
        macArtifact,
        windowsArtifact,
      ],
      { cwd: repositoryRoot, stdio: 'pipe' },
    );

    const verify = () =>
      execFileSync(
        process.execPath,
        [
          'scripts/endpoint_review.cjs',
          'verify',
          '--review',
          reviewPath,
          ...common,
          macArtifact,
          windowsArtifact,
        ],
        { cwd: repositoryRoot, stdio: 'pipe' },
      );
    expect(verify).not.toThrow();

    const review = JSON.parse(readFileSync(reviewPath, 'utf8'));
    review.artifacts[1] = { ...review.artifacts[0] };
    writeFileSync(reviewPath, `${JSON.stringify(review, null, 2)}\n`);
    expect(verify).toThrow(/duplicate artifact name/);

    execFileSync(
      process.execPath,
      [
        'scripts/endpoint_review.cjs',
        'record',
        '--output',
        reviewPath,
        ...common,
        '--status',
        'clear',
        '--reference',
        'submission-123',
        '--reviewer',
        'release-maintainer',
        '--reviewed-at',
        '2026-09-12',
        macArtifact,
        windowsArtifact,
      ],
      { cwd: repositoryRoot, stdio: 'pipe' },
    );
    const sizeTamperedReview = JSON.parse(readFileSync(reviewPath, 'utf8'));
    sizeTamperedReview.artifacts[1].bytes += 1;
    writeFileSync(reviewPath, `${JSON.stringify(sizeTamperedReview, null, 2)}\n`);
    expect(verify).toThrow(/size does not match the reviewed bytes/);
  });

  it('matches only explicit failed doctor rows in the Windows packaging smoke test', () => {
    const smokeTest = readFileSync(
      join(repositoryRoot, 'scripts/windows_packaging_smoke.ps1'),
      'utf8',
    );
    expect(smokeTest).toContain("$doctor.Output -match '^\\s*\\[FAIL\\](?:\\s|$)'");
    expect(smokeTest).not.toContain("$doctor.Output -match '(?m)\\bFAIL\\b'");
  });

  it('uses Node 24-based artifact actions in CI and release workflows', () => {
    const workflows = ['.github/workflows/ci.yml', '.github/workflows/release.yml']
      .map((workflow) => readFileSync(join(repositoryRoot, workflow), 'utf8'))
      .join('\n');

    expect(workflows).toContain('pnpm/action-setup@v6');
    expect(workflows).toContain('actions/upload-artifact@v7');
    expect(workflows).toContain('actions/download-artifact@v8');
    expect(workflows).not.toMatch(/pnpm\/action-setup@v4/);
    expect(workflows).not.toMatch(/actions\/upload-artifact@v4/);
    expect(workflows).not.toMatch(/actions\/download-artifact@v5/);
  });
});
