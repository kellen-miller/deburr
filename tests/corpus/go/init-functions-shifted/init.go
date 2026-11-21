//go:build ignore

package sample

// Lines inserted above must not change any identity.

// Another comment.

func init() {
	if true {
		return
	}
}


func init() {
	register(func() {})
}

func init() {
	if true {
		return
	}
}
