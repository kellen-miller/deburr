//go:build ignore

package sample

func Outer(value int) bool {
	if value > 0 {
		return true
	}
	return false
}

func Nested(value int) int {
	if value > 0 && value < 10 {
		inner := func() int {
			if value == 4 {
				return 1
			}
			return 2
		}
		return inner()
	}
	return 0
}
