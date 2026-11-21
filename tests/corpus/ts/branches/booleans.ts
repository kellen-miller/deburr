export function opposite(a: number, b: number): boolean {
  if (a === b) return true;
  return false;
}

export function negated(a: number, b: number): boolean {
  if (a !== b) { return false } else { return true }
}

export function doubleParenthesized(a: number, b: number): boolean {
  if ((a < b)) return true;
  return false;
}

export function notComparison(a: boolean): boolean {
  if (a) return true;
  return false;
}

export function sameLiteral(a: number): boolean {
  if (a > 1) return true;
  return true;
}

export function automatic(a: number): boolean {
  if (a > 1) return true
  return false
}

export const arrow = (a: number): boolean => {
  if (a >= 1) { return false; }
  return true;
};
