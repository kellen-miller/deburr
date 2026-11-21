//go:build ignore

package sample

func Parenthesized(a, b int) bool {
	if (a == b) {
		return true
	}
	return false
}

func Initializer(a int) bool {
	if x := a; x == 1 {
		return true
	}
	return false
}

func NotComparison(ok bool) bool {
	if ok {
		return true
	}
	return false
}

func Logical(a, b bool) bool {
	if a && b {
		return true
	}
	return false
}

func SameLiteral(a, b int) bool {
	if a == b {
		return true
	}
	return true
}

func HasElse(a, b int) bool {
	if a == b {
		return true
	} else {
		return false
	}
}

func Named(a, b int) (ok bool) {
	if a == b {
		return true
	}
	return false
}

func TwoResults(a, b int) (bool, error) {
	if a == b {
		return true, nil
	}
	return false, nil
}

func NotLiteral(a, b int, yes bool) bool {
	if a == b {
		return yes
	}
	return false
}

func Separated(a, b int) bool {
	if a == b {
		return true
	}
	a++
	return false
}

func InCaseBody(a int) bool {
	switch a {
	case 1:
		if a == 2 {
			return true
		}
		return false
	}
	return a > 3
}

func InSelectBody(a int, c chan int) bool {
	select {
	case <-c:
		if a == 2 {
			return true
		}
		return false
	default:
		if a == 3 {
			return true
		}
		return false
	}
}

var Literal = func(a, b int) bool {
	if a == b {
		return true
	}
	return false
}

func Wrapper(a, b int) bool {
	check := func() bool {
		if a == b {
			return true
		}
		return false
	}
	return check()
}
