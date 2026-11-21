//go:build ignore

package sample

func Deep(values []int, kind any, done chan struct{}) int {
	total := 0
	for _, value := range values {
		if value > 0 && total < 1000 {
			switch {
			case value > 10, value < 100:
				total++
			default:
				total--
			}
		} else if value < 0 {
			select {
			case <-done:
			default:
			}
		}
	}
	switch kind.(type) {
	case int, string:
	case nil:
	default:
	}
	return total
}
