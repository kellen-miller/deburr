//go:build ignore

package sample

func Same(a int) int {
	if a > 0 {
		a++
		a *= 2
	} else {
		a++
		a *= 2
	}
	return a
}

// Layout and comments do not change the syntax.
func Reformatted(a int) int {
	if a > 0 {
		a = a+1 // grow
	} else {
		a   =   a + 1
	}
	return a
}

func Different(a int) int {
	if a > 0 {
		a++
	} else {
		a--
	}
	return a
}

func ElseIf(a int) int {
	if a > 0 {
		a++
	} else if a < 0 {
		a++
	}
	return a
}

func ChainTail(a int) int {
	if a > 0 {
		a++
	} else if a < 0 {
		a--
	} else {
		a--
	}
	return a
}

func Empty(a int) {
	if a > 0 {
	} else {
	}
}

func Closure() func(int) int {
	return func(a int) int {
		if a > 0 {
			return 1
		} else {
			return 1
		}
	}
}

func Split(a int, g func()) {
	if a > 0 {
		return g()
	} else {
		return
		g()
	}
}
