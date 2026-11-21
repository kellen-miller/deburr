//go:build ignore

package sample

func Outer(value int) int {
	total := 0
	total += func() int {
		if value > 0 {
			return 1
		}
		return 2
	}()
	return total
}

func Declared()
