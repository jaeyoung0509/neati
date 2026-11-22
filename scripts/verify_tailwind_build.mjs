import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const assetsDir = join(process.cwd(), 'dist', 'assets');
const cssFiles = readdirSync(assetsDir).filter((file) => file.endsWith('.css'));
if (cssFiles.length === 0) {
  throw new Error('Tailwind verification could not find a generated CSS asset');
}

const css = cssFiles.map((file) => readFileSync(join(assetsDir, file), 'utf8')).join('\n');
const required = [
  '.duration-fast',
  '.status-panel-warning',
  '.focus-visible\\:ring-2',
  '.bg-background',
  '.liquid-sidebar',
  '.overview-hero',
  '--background:',
  '--ring:',
];
const missing = required.filter((marker) => !css.includes(marker));
if (missing.length > 0) {
  throw new Error(`Tailwind production CSS is missing: ${missing.join(', ')}`);
}

console.log(`Verified Tailwind utilities in ${cssFiles.length} production CSS asset(s).`);

// Preview fixtures must stay in their own lazily loaded chunk (#432): the
// entry and view chunks the packaged app always loads carry no fixture data.
const scripts = readdirSync(assetsDir).filter((file) => file.endsWith('.js'));
const fixtureMarker = '/Users/preview/';
const fixtureChunks = scripts.filter((file) =>
  readFileSync(join(assetsDir, file), 'utf8').includes(fixtureMarker),
);
if (fixtureChunks.length === 0) {
  throw new Error('Preview fixture chunk is missing; the fixture marker may have changed');
}
const leaked = fixtureChunks.filter((file) => !/^mock-/.test(file));
if (leaked.length > 0) {
  throw new Error(`Preview fixtures leaked into always-loaded chunks: ${leaked.join(', ')}`);
}
console.log(`Verified preview fixtures stay in ${fixtureChunks.join(', ')}.`);
