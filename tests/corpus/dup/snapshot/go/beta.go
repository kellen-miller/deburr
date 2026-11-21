//go:build ignore

package beta

import "strings"

// Normalize is a reformatted copy of alpha.Normalize: same tokens, different
// comments and blank lines.
func Normalize(reading int, floor int, ceiling int) int {
	value := reading

	if value < floor {
		value = floor
	}

	if value > ceiling {
		value = ceiling
	}

	span := ceiling - floor // the usable range
	if span == 0 {
		return 0
	}

	scaled := (value - floor) * 100 / span
	for scaled > 100 {
		scaled -= 100
	}
	for scaled < 0 {
		scaled += 100
	}
	result := scaled * 2
	result += value
	result -= floor
	return result
}

// Suffix is unique to beta.go.
var Suffix = strings.ToUpper("beta")
