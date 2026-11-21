import { readFileSync } from "node:fs";

export interface Entry {
  name: string;
  size: number;
}

export const DEFAULT_LIMIT = 10;

export function summarize(entries: Entry[], limit: number): string {
  const lines: string[] = [];
  let total = 0;
  for (const entry of entries) {
    if (entry.size > limit) {
      lines.push(`${entry.name} is large`);
    } else if (entry.size === 0) {
      lines.push(`${entry.name} is empty`);
    } else {
      lines.push(`${entry.name} is small`);
    }
    total += entry.size;
  }
  const label = total > limit * 2 ? "heavy" : "light";
  lines.push(`total ${total} (${label})`);
  return lines.join("\n");
}

export function load(path: string): string {
  return readFileSync(path, "utf8");
}
