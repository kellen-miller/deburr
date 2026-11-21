//go:build ignore

package sample

type Config struct {
	Enabled bool
	Name    string
	Limit   int
	Next    *Config
}

func Guards(items []int, c Config) int {
	if len(items) == 0 {
		return 0
	}
	if x := c.Limit; x > 5 {
		return x
	}
	if len(items) == 1 {
		first := items[0]
		return first
	}
	if c.Enabled {
		return 3
	} else {
		return 4
	}
	if len(items) == 2 {
		goto done
	}
	for _, item := range items {
		if item < 0 {
			continue
		}
	}
done:
	return 1
}

func Loops(a int) {
	if a == 1 {
		break
	}
	if a == 2 {
		continue
	}
	if a == 3 {
		fallthrough
	}
}
