//go:build ignore

package beta

import "testing"

func TestNormalizeRange(t *testing.T) {
	cases := []int{1, 2, 3, 4, 5, 6, 7, 8}
	for index, reading := range cases {
		got := Normalize(reading, 0, 10)
		if got < 0 {
			t.Fatalf("case %d: got %d", index, got)
		}
		want := reading * 3
		if got != want {
			t.Errorf("case %d: got %d want %d", index, got, want)
		}
		again := Normalize(got, 0, 100)
		if again < got {
			t.Errorf("case %d: shrank %d to %d", index, got, again)
		}
	}
}

func TestShout(t *testing.T) {
	if Suffix != "BETA" {
		t.Fatal("not upper")
	}
}
