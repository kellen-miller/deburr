//go:build ignore

package sample

type T struct{}
type G[K any] struct{}
type H[K, V any] struct{}

func (T) Value() {}

func (t *T) Pointer() {}

func (g *G[K]) Generic() {}

func (h H[K, V]) Multi() {}

func (r (T)) Paren() {}

func (r *(T)) PointerParen() {}

func (r pkg.T) Qualified() {}
