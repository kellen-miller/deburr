//go:build ignore

package sample

func Things(t *testing.T) {
	assert.Equal(t, 1, 1)
	assertValid(t)
	t.Fatal("bad")
}
