//go:build ignore

package sample

type Gate struct{ limit int }

func Equal(a, b int) bool {
	if a == b {
		return true
	}
	return false
}

func Less(a, b int) bool {
	if a+b*2   <  3 {
		return false
	}
	return true
}

func (g *Gate) Open(value int) (bool) {
	if value >= g.limit {
		return true
	}
	return false
}

func InLoop(values []int, limit int) bool {
	for _, value := range values {
		if value != limit {
			return true
		}
		return false
	}
	return false
}
