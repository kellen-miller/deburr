# Patched tree-sitter-typescript

- Upstream: https://github.com/tree-sitter/tree-sitter-typescript
- Version: 0.23.2, commit `f975a621f4e7f532fe322e13c4f79495e0a7b2e7`
- Upstream issue: https://github.com/tree-sitter/tree-sitter-typescript/issues/322
  (`import("x")` used as a type fails to parse; open, unfixed in 0.23.2)
- Parsers regenerated with tree-sitter CLI 0.24.7 (the version upstream pins;
  emits ABI 14, which the `tree-sitter = "0.27"` runtime loads). Regenerating
  the unpatched grammar with this CLI reproduces upstream's `parser.c` exactly;
  only `src/tree_sitter/array.h` differs.
- Lives under `third_party/`, not `vendor/`: a top-level `vendor/` directory
  switches the Go toolchain into module-vendoring mode and breaks `go build`.

The patch makes the grammar accept everything the Go version of deburr
(gotreesitter) accepts in VS Code `src` at
`bd64649be19f2da0f09c2ab84c066c6c764b83be` and the other real repositories
deburr is checked against. Three independent fixes:

## 1. `import("x")` as a type (issue #322)

    type A = (import("x").T);              // parenthesized import type
    type B = import("x").T[];              // array of import type
    const x: import("foo").Bar<Baz> = y;   // import type with type arguments
    f<typeof import("x")>();               // typeof import in call type arguments

- The import-type rules (`_type_query_member_expression_in_type_annotation`,
  `_type_query_call_expression_in_type_annotation`) were alternatives of
  `type` at `prec(-1)`, so array, lookup, union, parenthesized and generic
  types could not contain them. They move into `primary_type`, and the member
  form may name a `generic_type`. Two precedences prefer them over
  `primary_type`, so `.Name`, `(...)` and `<...>` extend the import type.
- Static precedences `[call_expression, _type_query_call_expression]`,
  `[call_expression, _type_query_call_expression_in_type_annotation]` and
  `[member_expression, _type_query_member_expression_in_type_annotation]`
  committed to the expression reading after `(` (parenthesized type vs
  function-type parameters) and `<` (type arguments vs less-than). They become
  GLR conflicts, so the input decides; the expression reading still wins where
  only it is valid (`a < import("x").length`, `(import("./m.js"))`).

## 2. Keywords as identifiers

    for (let i = 1; i < unique.length; i++) {}     // `unique` variable after `<`
    type T = [spoken: string, symbol: string][];   // tuple label named `symbol`

- `unique` (from the `unique symbol` type) joins `_reserved_identifier`, so
  where a type and an expression are both possible it can still be an
  identifier.
- Labeled tuple elements (`tuple_parameter`, `optional_tuple_parameter`)
  accept `_reserved_identifier` names (`string`, `symbol`, `type`, ...), with
  a conflict `[optional_tuple_parameter, predefined_type]` for `[any?]` vs
  `[any?: T]`.

## 3. Automatic semicolon before `<` in types (`common/scanner.h`)

    type S = {
      register: {
        (plugin: P): () => void
        <T extends R<string, object>>(plugin: P<T>): string
      }
    }

The external scanner never inserted an automatic semicolon before a `<` on
the next line, so a generic call signature could not follow a member that
ends without a separator. It now treats `<` like `(` and `[`: no semicolon
when a binary operator is valid (an expression, so `a\n< b` is unchanged),
otherwise insert one. TypeScript itself never reads type arguments across a
line break.

## Effects

- Upstream corpus: all 112 cases pass unchanged in both dialects. New cases
  in `test/corpus/deburr_patches.txt` (8 cases) pass in both dialects.
- Files with ERROR/MISSING nodes, before -> after: VS Code `src` (10,045
  files) 14 -> 0; chief 4 -> 0; 3,062 other local TypeScript files 4 -> 0.
- Trees of files that already parsed change only for `f<typeof import("x")>(args)`
  calls, which 0.23.2 mis-read as two `<`/`>` binary expressions and now read
  as calls with type arguments (32 VS Code files, 3 others).
- `node-types.json`: `member_expression` and `call_expression` move from the
  `type` supertype to `primary_type`; `generic_type.name` may be a
  `member_expression`.

## Diff (`common/`)

```diff
diff --git a/common/define-grammar.js b/common/define-grammar.js
index efb0b76..49b9df1 100644
--- a/common/define-grammar.js
+++ b/common/define-grammar.js
@@ -52,7 +52,6 @@ module.exports = function defineGrammar(dialect) {
       [$.nested_type_identifier, $.generic_type, $.primary_type, $.lookup_type, $.index_type_query, $.type],
       [$.as_expression, $.satisfies_expression, $.primary_type],
       [$._type_query_member_expression, $.member_expression],
-      [$.member_expression, $._type_query_member_expression_in_type_annotation],
       [$._type_query_member_expression, $.primary_expression],
       [$._type_query_subscript_expression, $.subscript_expression],
       [$._type_query_subscript_expression, $.primary_expression],
@@ -63,10 +62,10 @@ module.exports = function defineGrammar(dialect) {
       [$.decorator_call_expression, $.decorator],
       [$.literal_type, $.pattern],
       [$.predefined_type, $.pattern],
-      [$.call_expression, $._type_query_call_expression],
-      [$.call_expression, $._type_query_call_expression_in_type_annotation],
       [$.new_expression, $.primary_expression],
       [$.meta_property, $.primary_expression],
+      [$._type_query_call_expression_in_type_annotation, $.primary_type],
+      [$._type_query_member_expression_in_type_annotation, $.primary_type],
       [$.construct_signature, $._property_name],
     ]),
 
@@ -98,6 +97,7 @@ module.exports = function defineGrammar(dialect) {
       [$.pattern, $.primary_type],
 
       [$.optional_tuple_parameter, $.primary_type],
+      [$.optional_tuple_parameter, $.predefined_type],
       [$.rest_pattern, $.primary_type, $.primary_expression],
 
       [$.object, $.object_type],
@@ -111,6 +111,10 @@ module.exports = function defineGrammar(dialect) {
       [$.array_pattern, $.tuple_type],
 
       [$.template_literal_type, $.template_string],
+
+      [$.call_expression, $._type_query_call_expression],
+      [$.call_expression, $._type_query_call_expression_in_type_annotation],
+      [$.member_expression, $._type_query_member_expression_in_type_annotation],
     ]).concat(
       dialect === 'typescript' ? [
         [$.primary_type, $.type_parameter],
@@ -726,17 +730,22 @@ module.exports = function defineGrammar(dialect) {
         $.readonly_type,
         $.constructor_type,
         $.infer_type,
-        prec(-1, alias($._type_query_member_expression_in_type_annotation, $.member_expression)),
-        prec(-1, alias($._type_query_call_expression_in_type_annotation, $.call_expression)),
       ),
 
       tuple_parameter: $ => seq(
-        field('name', choice($.identifier, $.rest_pattern)),
+        field('name', choice(
+          $.identifier,
+          alias($._reserved_identifier, $.identifier),
+          $.rest_pattern,
+        )),
         field('type', $.type_annotation),
       ),
 
       optional_tuple_parameter: $ => seq(
-        field('name', $.identifier),
+        field('name', choice(
+          $.identifier,
+          alias($._reserved_identifier, $.identifier),
+        )),
         '?',
         field('type', $.type_annotation),
       ),
@@ -782,6 +791,8 @@ module.exports = function defineGrammar(dialect) {
         $.intersection_type,
         $.union_type,
         'const',
+        alias($._type_query_member_expression_in_type_annotation, $.member_expression),
+        alias($._type_query_call_expression_in_type_annotation, $.call_expression),
       ),
 
       template_type: $ => seq('${', choice($.primary_type, $.infer_type), '}'),
@@ -818,6 +829,7 @@ module.exports = function defineGrammar(dialect) {
         field('name', choice(
           $._type_identifier,
           $.nested_type_identifier,
+          alias($._type_query_member_expression_in_type_annotation, $.member_expression),
         )),
         field('type_arguments', $.type_arguments),
       )),
@@ -1092,6 +1104,7 @@ module.exports = function defineGrammar(dialect) {
         'object',
         'new',
         'readonly',
+        'unique',
         previous,
       ),
     },
diff --git a/common/scanner.h b/common/scanner.h
index 23b78b5..5b2322e 100644
--- a/common/scanner.h
+++ b/common/scanner.h
@@ -123,7 +123,6 @@ static bool scan_automatic_semicolon(TSLexer *lexer, const bool *valid_symbols,
         case '*':
         case '%':
         case '>':
-        case '<':
         case '=':
         case '?':
         case '^':
@@ -139,11 +138,14 @@ static bool scan_automatic_semicolon(TSLexer *lexer, const bool *valid_symbols,
             }
             break;
 
-            // Don't insert a semicolon before a '[' or '(', unless we're parsing
-            // a type. Detect whether we're parsing a type or an expression using
-            // the validity of a binary operator token.
+            // Don't insert a semicolon before a '[', '(' or '<', unless we're
+            // parsing a type. Detect whether we're parsing a type or an expression
+            // using the validity of a binary operator token. In a type, '<' after
+            // a line break starts the next member's type parameters: TypeScript
+            // does not read type arguments across a line break.
         case '(':
         case '[':
+        case '<':
             if (valid_symbols[LOGICAL_OR]) {
                 return false;
             }
```

## Regenerating

    git clone https://github.com/tree-sitter/tree-sitter-typescript && cd tree-sitter-typescript
    git checkout f975a621f4e7f532fe322e13c4f79495e0a7b2e7 && npm ci --ignore-scripts
    cp <this dir>/common/define-grammar.js <this dir>/common/scanner.h common/
    cp <this dir>/test/corpus/deburr_patches.txt test/corpus/
    (cd typescript && tree-sitter generate) && (cd tsx && tree-sitter generate)   # CLI 0.24.7
    tree-sitter test   # typescript is the default; reverse the grammars in tree-sitter.json to test tsx

Then copy `typescript/src` and `tsx/src` back here. Drop this copy once an
upstream release fixes these.
