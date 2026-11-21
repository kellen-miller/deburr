export function bare(flag: boolean, log: (n: number) => void) {
  if (flag) log(1); else log(1);
  if (flag) log(2);
  else log(2);
}

export function blocks(flag: boolean, log: (n: number) => void) {
  if (flag) { log(1); } else { log(1); }
  if (flag) { log(2) } else { log(3) }
  if (flag) {} else {}
  if (flag) { /* a */ } else { /* b */ }
  if (flag) { log(4) } else if (!flag) { log(4) }
}

export function sibling(flag: boolean, log: (n: number) => void) {
  if (flag) { log(5); }
  { log(5); }
  if (flag) { log(6); }
  log(6);
}
