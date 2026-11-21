//go:build ignore

package sample

func Production(value int) int {
	if value > 0 {
		return value
	}
	return 0
}
