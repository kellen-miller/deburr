abstract class A { run() { const nested = () => 1; return nested(); } }
abstract class B { run() { return 2; } }
function factory() {
  class Inner { run() { return 3; } }
  return Inner;
}
function run() { return 4; }
