//go:build ignore

package sample

func Decisions(value int) int {
	if value > 0 && value < 10 {
		return 1
	}
	switch value {
	default:
		return 0
	}
}
