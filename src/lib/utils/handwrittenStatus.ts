export type LoadingWord = 'loading' | 'scanning' | 'cleaning' | 'working';
export type LoadingTone = 'brand' | 'ink';
export type LoadingMotion = 'write' | 'flow';

interface Glyph { width: number; path: string; dot?: [number, number] }
const glyphs: Record<string, Glyph> = {
  l: { width: 26, path: 'M0 43 C9 35 27 12 21 7 C14 1 7 29 8 40 C9 51 18 46 26 36' },
  o: { width: 26, path: 'M0 36 C5 27 18 23 18 33 C17 45 4 49 2 40 C0 33 8 26 15 29 C18 33 20 34 26 32' },
  a: { width: 29, path: 'M0 32 C6 25 20 25 17 33 C14 46 0 48 0 39 C1 30 13 25 18 29 L14 41 C12 49 22 45 29 36' },
  d: { width: 31, path: 'M0 36 C5 27 18 25 16 34 C14 45 -1 48 -1 40 C0 31 12 27 17 29 C23 20 32 4 28 5 C21 7 17 28 13 40 C11 50 22 44 31 34' },
  i: { width: 13, path: 'M0 34 L-3 42 C-5 49 5 44 13 30', dot: [3, 19] },
  n: { width: 28, path: 'M0 30 L-4 43 C-1 34 9 25 13 30 C17 35 4 48 14 44 C20 41 22 35 28 31' },
  g: { width: 31, path: 'M0 31 C5 26 16 26 14 34 C11 47 -3 49 -3 40 C-2 31 10 27 15 29 L7 52 C3 64 -8 59 -3 53 C3 46 17 47 31 38' },
  c: { width: 24, path: 'M0 36 C7 22 25 26 17 31 C11 25 -2 33 0 41 C3 50 17 46 24 36' },
  e: { width: 23, path: 'M0 36 C24 35 20 18 8 24 C-4 31 -4 47 8 47 Q16 47 23 40' },
  s: { width: 27, path: 'M0 37 C8 32 17 24 16 27 C11 34 12 37 16 39 C23 44 8 51 2 43 C5 48 17 46 27 36' },
  w: { width: 36, path: 'M0 30 L-3 42 C-6 52 6 43 13 29 L9 41 C6 52 20 44 25 29 C28 35 31 36 36 32' },
  r: { width: 21, path: 'M0 34 L-4 44 L3 28 C6 23 10 29 8 32 C12 36 16 35 21 31' },
  k: { width: 25, path: 'M0 43 C8 27 23 6 18 5 C12 7 6 29 1 44 C5 35 20 25 21 30 C22 35 7 39 4 37 C9 38 12 50 18 44 L25 36' },
};

function buildWord(word: LoadingWord) {
  let x = 5;
  const strokes: string[] = [];
  const dots: string[] = [];
  for (const letter of word) {
    const glyph = glyphs[letter];
    let first = true;
    strokes.push(glyph.path.replace(/([MLCQ])([^MLCQ]*)/g, (_segment, command: string, coordinates: string) => {
      const values = coordinates.trim().split(/[ ,]+/).map(Number)
        .map((value, index) => index % 2 === 0 ? value + x : value);
      const nextCommand = first && strokes.length ? 'L' : command;
      first = false;
      return `${nextCommand}${values.join(' ')} `;
    }).trim());
    if (glyph.dot) dots.push(`M${x + glyph.dot[0]} ${glyph.dot[1]} L${x + glyph.dot[0] + 0.5} ${glyph.dot[1] - 1}`);
    x += glyph.width;
  }
  return { path: strokes.join(' '), dots: dots.join(' '), width: x + 7 };
}

/** Original local pen geometry; no font download, arbitrary copy, or brand asset mutation. */
export const handwrittenStatus = {
  loading: buildWord('loading'), scanning: buildWord('scanning'),
  cleaning: buildWord('cleaning'), working: buildWord('working'),
} satisfies Record<LoadingWord, { path: string; dots: string; width: number }>;
