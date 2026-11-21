//go:build ignore

package alpha

// Normalize clamps and rescales a reading. Its body is copied into beta.go.
func Normalize(reading int, floor int, ceiling int) int {
	value := reading
	if value < floor {
		value = floor
	}
	if value > ceiling {
		value = ceiling
	}
	span := ceiling - floor
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

// Label is unique to alpha.go.
func Label(code int) string {
	if code == 0 {
		return "none"
	}
	return "some"
}
