//go:build ignore

package sample

func Outer() {
	_ = func() int { return 1 }
	_ = func() int { return 1 }
	_ = func() int { return 2 }
}
