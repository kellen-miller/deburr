export const arrow = () => 1;
export const expression = function () { return 2; };
export const named = function inner() { return 3; };
const holder = { method() { return 4; }, prop: () => 5 };

class Box {
  value = () => 6;
  static make() { return new Box(); }
  get size() { return 7; }
  constructor() { const local = () => 8; local(); }
}

function outer() {
  return [1, 2].map((n) => n * 2);
}

function repeated() { return 1; }
function repeated() { return 2; }
