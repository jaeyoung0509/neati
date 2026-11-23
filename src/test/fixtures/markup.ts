/** The opening `<button …>` tag of the first button whose content contains `label`. */
export function buttonTag(body: string, label: string): string {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = body.match(new RegExp(`<button[^>]*>(?:(?!</button>)[\\s\\S])*?${escaped}`));
  if (!match) throw new Error(`No button labelled ${label}`);
  return match[0].slice(0, match[0].indexOf('>') + 1);
}

/** A real `disabled` attribute, not a `disabled:` utility class. */
export function isDisabled(tag: string): boolean {
  return /\sdisabled(?=[\s>=/])/.test(tag);
}

/** The opening `<input …>` tag carrying `aria-label="…label"`. */
export function inputTags(body: string, ariaLabel: RegExp): string[] {
  return [...body.matchAll(/<input[^>]*>/g)].map((match) => match[0]).filter((tag) => {
    const label = tag.match(/aria-label="([^"]*)"/)?.[1];
    return label !== undefined && ariaLabel.test(label);
  });
}
