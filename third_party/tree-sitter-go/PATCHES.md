# Patched tree-sitter-go

- Upstream: https://github.com/tree-sitter/tree-sitter-go
- Version: 0.25.0, commit `1547678a9da59885853f5f5cc8a99cc203fa2e2c` (latest
  release; neither shape below is supported upstream yet)
- Parser regenerated with tree-sitter CLI 0.25.8 (the version upstream used;
  emits ABI 15, which the `tree-sitter = "0.27"` runtime loads). Regenerating
  the unpatched grammar with this CLI reproduces upstream's `src/` exactly.
  CLI 0.24.7 does not: it drops the grammar's `reserved` words and emits ABI 14.
- Lives under `third_party/`, not `vendor/`: a top-level `vendor/` directory
  switches the Go toolchain into module-vendoring mode and breaks `go build`.

The patch makes the grammar accept the Go 1.26 and 1.27 syntax that the Go
version of deburr (go/parser from Go 1.27) accepts. Two independent fixes:

## 1. `new(expr)` (Go 1.26)

    val := new(42)
    v := new(int64(-64))
    NewConnContextKey = new("NewConnContextKey")
    s := new(secretType(secretValue))

`new` and `make` take `special_argument_list`, whose first argument had to be
a type. It may now also be an expression, at `prec.dynamic(-10)`, so where
both readings fit (`new(T)`, `new(pkg.T)`, `new(G[int])`, `new(*T)`) the type
reading still wins and those trees are unchanged. The argument list is still
aliased to `argument_list`; its first child may now be any expression.

## 2. Generic methods (Go 1.27)

    func (r *Rand) N[Int intType](n Int) Int { ... }
    func (t testTracebackGenericTyp[P]) G[R any](buf []byte) R { ... }

`method_declaration` gains an optional `type_parameters` field (a
`type_parameter_list`) between `name` and `parameters`, matching
`function_declaration`. A conflict `[type_parameter_declaration, _simple_type]`
lets GLR separate it from a top-level function type with a generic result
(`func(r T) N[A, B](x)`, still a `type_conversion_expression`). Interface
methods and function literals stay without type parameters, as in Go 1.27.

## Effects

- Upstream corpus: all 67 cases pass unchanged. New cases in
  `test/corpus/deburr_patches.txt` (4 cases) pass.
- `GOROOT/src` of Go 1.27.4, excluding `testdata` (6,828 files, all accepted
  by go/parser): files with ERROR/MISSING nodes 7 -> 0.
- Trees change only in those 7 files; every file that parsed before has an
  identical tree.
- `node-types.json`: `method_declaration` gains the optional `type_parameters`
  field. No other node or field changes.

## Diff (`grammar.js`)

```diff
@@ -93,6 +93,7 @@ module.exports = grammar({
     [$.type_parameter_declaration, $._simple_type, $._expression],
     [$.type_parameter_declaration, $._expression],
     [$.type_parameter_declaration, $._simple_type, $.generic_type, $._expression],
+    [$.type_parameter_declaration, $._simple_type],
   ],
 
   reserved: {
@@ -231,6 +232,7 @@ module.exports = grammar({
       'func',
       field('receiver', $.parameter_list),
       field('name', $._field_identifier),
+      field('type_parameters', optional($.type_parameter_list)),
       field('parameters', $.parameter_list),
       field('result', optional(choice($.parameter_list, $._simple_type))),
       field('body', optional($.block)),
@@ -709,7 +711,7 @@ module.exports = grammar({
     special_argument_list: $ => seq(
       '(',
       optional(seq(
-        $._type,
+        choice($._type, prec.dynamic(-10, $._expression)),
         repeat(seq(',', $._expression)),
         optional(','),
       )),
```

## Regenerating

    git clone https://github.com/tree-sitter/tree-sitter-go && cd tree-sitter-go
    git checkout 1547678a9da59885853f5f5cc8a99cc203fa2e2c
    cp <this dir>/grammar.js .
    cp <this dir>/test/corpus/deburr_patches.txt test/corpus/
    tree-sitter generate && tree-sitter test   # CLI 0.25.8

Then copy `src` back here. Drop this copy once an upstream release supports
these.
