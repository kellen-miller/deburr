//! The patched tree-sitter-go grammar accepts `new(expr)` (Go 1.26) and
//! generic methods (Go 1.27). Each shape comes from a real `GOROOT/src` file
//! that the published 0.25.0 grammar rejected; see
//! `third_party/tree-sitter-go/PATCHES.md`.

use deburr::syntax::{self, Language, SyntaxError};

/// A source snippet and the shape it exercises.
struct Shape {
    name: &'static str,
    source: &'static str,
}

const SHAPES: [Shape; 8] = [
    Shape {
        name: "new with an int literal",
        source: "package p\n\nfunc f() {\n\tval := new(42)\n\t_ = val\n}\n",
    },
    Shape {
        name: "new with a conversion",
        source: "package p\n\nvar v = new(int64(-64))\n",
    },
    Shape {
        name: "new with a string literal",
        source: "package p\n\nvar (\n\tNewConnContextKey = new(\"NewConnContextKey\")\n)\n",
    },
    Shape {
        name: "new with a call of a named type",
        source: "package p\n\nfunc f() {\n\ts := new(secretType(secretValue))\n\t_ = s\n}\n",
    },
    Shape {
        name: "new and make with type operands",
        source: "package p\n\nvar a, b, c = new(T), new(pkg.T[int]), make([]int, n)\n",
    },
    Shape {
        name: "generic method on a pointer receiver",
        source: "package p\n\nfunc (r *Rand) N[Int intType](n Int) Int {\n\treturn n\n}\n",
    },
    Shape {
        name: "generic method on a generic receiver",
        source: "package p\n\nfunc (t testTracebackGenericTyp[P]) G[R any](buf []byte) R {\n\t\
                 var r R\n\treturn r\n}\n",
    },
    Shape {
        name: "function type with a generic result converting a value",
        source: "package p\n\nvar v = func(r T) N[A, B](x)\n",
    },
];

#[test]
fn go_1_26_and_1_27_shapes_parse() {
    for shape in &SHAPES {
        let result = syntax::parse(Language::Go, shape.source.as_bytes().to_vec());

        if let Err(error) = result {
            panic!("Go rejected {}: {error}", shape.name);
        }
    }
}

#[test]
fn genuine_syntax_error_still_fails() {
    let result = syntax::parse(
        Language::Go,
        b"package p\n\nfunc (r T) M[P any(x P) {}\n".to_vec(),
    );

    assert!(
        matches!(result, Err(SyntaxError::Invalid { .. })),
        "Go accepted a broken generic method"
    );
}
