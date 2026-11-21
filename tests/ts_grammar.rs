//! The patched tree-sitter-typescript grammar accepts `import("x")` used as a
//! type (upstream issue #322), keywords used as identifiers, and generic call
//! signatures after a line break, in both the TypeScript and TSX dialects.
//! Each shape comes from a real file that the published 0.23.2 grammar
//! rejected; see `third_party/tree-sitter-typescript/PATCHES.md`.

use deburr::syntax::{self, Language, SyntaxError};

const DIALECTS: [Language; 2] = [Language::TypeScript, Language::Tsx];

/// A source snippet and the shape it exercises.
struct Shape {
    name: &'static str,
    source: &'static str,
}

const SHAPES: [Shape; 12] = [
    Shape {
        name: "parenthesized import type",
        source: "type A = (import(\"x\").T);\n",
    },
    Shape {
        name: "import type array",
        source: "type B = import(\"x\").T[];\n",
    },
    Shape {
        name: "import type with type arguments",
        source: "const x: import('foo').Bar<Baz> = y;\n",
    },
    Shape {
        name: "typeof import in call type arguments",
        source: "f<typeof import('x')>();\n",
    },
    Shape {
        name: "typeof import in awaited spread call",
        source: "vi.mock('@openai/agents', async (importOriginal) => ({\n  \
                 ...(await importOriginal<typeof import('@openai/agents')>()),\n}));\n",
    },
    Shape {
        name: "declared function returning an import type array",
        source: "export declare function buildCommandDefinitions(): \
                 import(\"discord.js\").SlashCommandOptionsOnlyBuilder[];\n",
    },
    Shape {
        name: "parenthesized union of import type and object type",
        source: "export interface ContextPayload {\n    historicalContext: \
                 (import(\"./context-types.js\").HistoricalRollupContext | {\n        \
                 kind: \"none\";\n    })[];\n}\n",
    },
    Shape {
        name: "parenthesized union of generic import types",
        source: "export declare function tools(): \
                 (import(\"@openai/agents/realtime\").FunctionTool<unknown, z.ZodObject<{\n    \
                 query: z.ZodString;\n}, z.core.$strip>, string> | \
                 import(\"@openai/agents/realtime\").FunctionTool<unknown>)[];\n",
    },
    Shape {
        name: "unique as an identifier after less-than",
        source: "for (let i = 1; i < unique.length; i++) {\n    use(i);\n}\n",
    },
    Shape {
        name: "labeled tuple element named like a keyword",
        source: "const symbolMap: [spoken: string, symbol: string][] = [];\n",
    },
    Shape {
        name: "generic call signature after a line break",
        source: "export type Slots = {\n  register: {\n    (plugin: Plugin): () => void\n    \
                 <S extends Record<string, object>>(plugin: Plugin<S>): () => void\n  }\n}\n",
    },
    Shape {
        name: "import call compared in an expression",
        source: "if (a < import(\"x\").length) {\n    b();\n}\n",
    },
];

#[test]
fn import_type_shapes_parse_in_every_dialect() {
    for language in DIALECTS {
        for shape in &SHAPES {
            let result = syntax::parse(language, shape.source.as_bytes().to_vec());

            if let Err(error) = result {
                panic!("{language:?} rejected {}: {error}", shape.name);
            }
        }
    }
}

#[test]
fn genuine_syntax_error_still_fails() {
    for language in DIALECTS {
        let result = syntax::parse(language, b"type A = import(\"x\").;\n".to_vec());

        assert!(
            matches!(result, Err(SyntaxError::Invalid { .. })),
            "{language:?} accepted a broken import type"
        );
    }
}
