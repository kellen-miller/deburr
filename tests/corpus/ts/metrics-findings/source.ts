// A comment.
export function check(value: number): boolean {
  const nested = (n: number) => {
    while (n > 0) { n--; }
    return n;
  };
  if (value > 0 && value < 10) { return true; } else { return false; }
}
class Service {
  run(value: number) {
    if (value > 0) { console.log(value); } else { console.log(value); }
  }
}
