//go:build ignore

package sample

//line virtual.go:100
func Directive(value int) bool {
	if value > 0 {
		return true
	}
	return false
}
