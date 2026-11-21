//go:build ignore

package sample

func TestThings(t *testing.T) {
	assert.Equal(t, 1, 1)
	require.NoError(t, err)
	assertValid(t)
	expectOK(t)
	t.Errorf("bad")
	t.Fatal("bad")
	testify.Foo()
	generic.Assert[int, string](t)
	assert.Equal[int](t, 1, 2)
	t.Log("fine")
	other.Errorf("no")
	TestHelper(t)
	t.Run("sub", func(t *testing.T) {
		t.Fatalf("inner")
	})
}
