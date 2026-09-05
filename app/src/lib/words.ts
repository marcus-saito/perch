/**
 * Counts as a person would say them. Under thirteen a count is a word, above
 * that a numeral, which is the rule the design language sets for every
 * sentence the interface writes. Nothing here is a score: these are counts of
 * fields, boxes and roles.
 */
const WORDS = [
  "no",
  "one",
  "two",
  "three",
  "four",
  "five",
  "six",
  "seven",
  "eight",
  "nine",
  "ten",
  "eleven",
  "twelve",
];

/** "no", "one", "two" … "twelve", then "13", "14" and so on. */
export function count(n: number): string {
  return WORDS[n] ?? String(n);
}

/** The same, for the start of a sentence. */
export function Count(n: number): string {
  const word = count(n);
  return word.charAt(0).toUpperCase() + word.slice(1);
}
