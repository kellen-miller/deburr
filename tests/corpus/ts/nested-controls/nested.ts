function nested(n: number) {
  if (n > 0) {
    for (let i = 0; i < n; i++) {
      while (n > 1) { n--; }
      switch (n) { case 1: break; }
    }
  }
}
