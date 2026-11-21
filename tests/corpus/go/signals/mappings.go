//go:build ignore

package sample

func Build(name string, c Config) []Config {
	a := Config{Enabled: true, Name: name, Limit: c.Limit, Next: nil, Count: iota, Raw: 5}
	b := pkg.Config{Name: name}
	s := struct{ A int }{A: name}
	list := []Config{{Name: name}}
	m := map[string]string{name: name}
	g := Generic[int]{Value: name}
	n := Config{Next: &Config{Name: name}}
	f := func() Config {
		return Config{Name: name}
	}
	return list
}
