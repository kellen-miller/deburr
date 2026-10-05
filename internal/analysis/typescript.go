package analysis

import (
	"fmt"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
	"github.com/odvcencio/gotreesitter"
	"github.com/odvcencio/gotreesitter/grammars/tsx"
	"github.com/odvcencio/gotreesitter/grammars/typescript"
)

type typescriptFunction struct {
	node   *gotreesitter.Node
	parent *typescriptFunction
	metric report.Function
}

func parseTypeScript(path string, source []byte) (*gotreesitter.Tree, *gotreesitter.Language, error) {
	language := typescript.Language()
	if filepath.Ext(path) == ".tsx" {
		language = tsx.Language()
	}

	parser := gotreesitter.NewParser(language)
	parser.SetTimeoutMicros(2_000_000)
	tree, err := parser.ParseStrict(source)
	if err != nil {
		if tree != nil {
			tree.Release()
		}

		return nil, language, fmt.Errorf("parse TypeScript: %w", err)
	}

	if tree.RootNode().HasErrorOrMissing() {
		tree.Release()
		return nil, language, fmt.Errorf("parse TypeScript %s: invalid syntax", path)
	}

	return tree, language, nil
}

func analyzeTypeScript(file *sourceFile) ([]report.Function, []report.Finding, error) {
	tree, language, err := parseTypeScript(file.path, file.source)
	if err != nil {
		return nil, nil, err
	}

	defer tree.Release()
	root := tree.RootNode()
	functions := make([]*typescriptFunction, 0)
	var collect func(*gotreesitter.Node, *typescriptFunction)
	collect = func(node *gotreesitter.Node, parent *typescriptFunction) {
		if typescriptFunctionNode(node, language) && node.ChildByFieldName("body", language) != nil {
			name := ""
			if identifier := node.ChildByFieldName("name", language); identifier != nil {
				name = identifier.Text(file.source)
			} else if owner := node.Parent(); owner != nil {
				if identifier := owner.ChildByFieldName("name", language); identifier != nil {
					name = identifier.Text(file.source)
				}
			}

			fingerprint := "sha256:" + digestText(typescriptNodeTokens(node, language, file.source))
			if name == "" {
				name = "anon-" + strings.TrimPrefix(fingerprint, "sha256:")
			}

			if parent != nil {
				name = parent.metric.Name + "." + name
			} else {
				for owner := node.Parent(); owner != nil; owner = owner.Parent() {
					if owner.Type(language) == "class_declaration" || owner.Type(language) == "class" {
						if identifier := owner.ChildByFieldName("name", language); identifier != nil {
							name = identifier.Text(file.source) + "." + name
						}

						break
					}
				}
			}

			start, end := typescriptPosition(node, file.source)
			function := &typescriptFunction{node: node, parent: parent, metric: report.Function{
				ID: "typescript/" + file.path + "::" + name, Name: name, Fingerprint: fingerprint,
				Path: file.path, Category: file.category, Start: start, End: end, Cyclomatic: 1,
			}}
			functions = append(functions, function)
			parent = function
		}

		for index := 0; index < node.NamedChildCount(); index++ {
			collect(node.NamedChild(index), parent)
		}
	}

	collect(root, nil)
	counts := make(map[string]int)
	for _, function := range functions {
		counts[function.metric.ID]++
	}

	occurrences := make(map[string]int)
	findings := make([]report.Finding, 0)
	for _, function := range functions {
		base := function.metric.ID
		function.metric.IdentityAmbiguous = counts[base] > 1 || (function.parent != nil && function.parent.metric.IdentityAmbiguous)
		occurrences[base]++
		if occurrences[base] > 1 {
			function.metric.ID += "#" + strconv.Itoa(occurrences[base])
		}

		var walk func(*gotreesitter.Node, int)
		walk = func(node *gotreesitter.Node, depth int) {
			if node != function.node && typescriptFunctionNode(node, language) {
				return
			}

			switch node.Type(language) {
			case "if_statement", "for_statement", "for_in_statement", "while_statement", "do_statement", "catch_clause", "ternary_expression":
				function.metric.Cyclomatic++
				depth++
				if node.Type(language) == "if_statement" && depth > 1 {
					function.metric.StructuralSignals.NestedBranches++
				}

			case "switch_statement":
				depth++
			case "switch_case":
				function.metric.Cyclomatic++
			case "binary_expression":
				if operator := node.ChildByFieldName("operator", language); operator != nil {
					switch operator.Text(file.source) {
					case "&&", "||", "??":
						function.metric.Cyclomatic++
					}
				}
			}

			if depth > function.metric.MaxNesting {
				function.metric.MaxNesting = depth
			}

			findings = append(findings, typescriptBranchFindings(file, function, node, language)...)
			for index := 0; index < node.NamedChildCount(); index++ {
				walk(node.NamedChild(index), depth)
			}
		}

		walk(function.node.ChildByFieldName("body", language), 0)
	}

	// Assign each code line to its innermost function, matching Go accounting.
	for line, code := range file.lineCode {
		if !code {
			continue
		}

		var owner *typescriptFunction
		for _, function := range functions {
			if line < function.metric.Start.Line || line > function.metric.End.Line {
				continue
			}

			if owner == nil || function.node.EndByte()-function.node.StartByte() < owner.node.EndByte()-owner.node.StartByte() {
				owner = function
			}
		}

		if owner != nil {
			owner.metric.SLOC++
		}
	}

	metrics := make([]report.Function, 0, len(functions))
	for _, function := range functions {
		function.metric.Mass = functionMass(function.metric.Cyclomatic, function.metric.SLOC)
		function.metric.HighComplexity = function.metric.Cyclomatic > highComplexityThreshold
		metrics = append(metrics, function.metric)
	}

	findingCounts := make(map[string]int)
	for index := range findings {
		base := findings[index].ID
		findingCounts[base]++
		if findingCounts[base] > 1 {
			findings[index].ID += "#" + strconv.Itoa(findingCounts[base])
		}
	}

	return metrics, findings, nil
}

func typescriptFunctionNode(node *gotreesitter.Node, language *gotreesitter.Language) bool {
	switch node.Type(language) {
	case "function_declaration", "generator_function_declaration", "function_expression", "generator_function", "arrow_function", "method_definition":
		return true
	default:
		return false
	}
}

func typescriptPosition(node *gotreesitter.Node, source []byte) (report.Position, report.Position) {
	point := node.StartPoint()
	start := report.Position{Line: int(point.Row) + 1, Column: int(point.Column) + 1}
	endPoint := node.EndPoint()
	end := report.Position{Line: int(endPoint.Row) + 1, Column: int(endPoint.Column)}
	if end.Column == 0 && end.Line > 1 {
		end.Line--
		end.Column = sourceLineLength(source, end.Line)
	}

	return start, end
}

type typescriptSourceToken struct {
	start, end uint32
	text       string
}

// Preserve literals verbatim; only syntax whitespace and comments are ignored.
func typescriptTokens(node *gotreesitter.Node, language *gotreesitter.Language, source []byte) []typescriptSourceToken {
	tokens := make([]typescriptSourceToken, 0)
	var walk func(*gotreesitter.Node)
	walk = func(current *gotreesitter.Node) {
		typeName := current.Type(language)
		if typeName == "comment" {
			return
		}

		if current.ChildCount() == 0 || typeName == "string" || typeName == "template_string" || typeName == "regex" {
			tokens = append(tokens, typescriptSourceToken{current.StartByte(), current.EndByte(), current.Text(source)})
			return
		}

		for index := 0; index < current.ChildCount(); index++ {
			walk(current.Child(index))
		}
	}

	walk(node)
	return tokens
}

func typescriptNodeTokens(node *gotreesitter.Node, language *gotreesitter.Language, source []byte) string {
	var normalized strings.Builder
	for _, token := range typescriptTokens(node, language, source) {
		normalized.WriteString(strconv.Quote(token.text))
		normalized.WriteByte(';')
	}

	return normalized.String()
}

func typescriptLineMap(path string, source []byte) []bool {
	lines := make([]bool, physicalLineCount(source)+1)
	tree, language, err := parseTypeScript(path, source)
	if err != nil {
		return lines
	}

	defer tree.Release()
	var walk func(*gotreesitter.Node)
	walk = func(node *gotreesitter.Node) {
		if node.Type(language) == "comment" {
			return
		}

		if node.ChildCount() == 0 {
			start, end := typescriptPosition(node, source)
			for line := start.Line; line <= end.Line && line < len(lines); line++ {
				lines[line] = true
			}

			return
		}

		for index := 0; index < node.ChildCount(); index++ {
			walk(node.Child(index))
		}
	}

	walk(tree.RootNode())
	return lines
}

func typescriptBranchFindings(file *sourceFile, function *typescriptFunction, node *gotreesitter.Node, language *gotreesitter.Language) []report.Finding {
	if node.Type(language) != "if_statement" {
		return nil
	}

	consequence := node.ChildByFieldName("consequence", language)
	alternative := node.ChildByFieldName("alternative", language)
	if consequence == nil {
		return nil
	}

	if alternative == nil {
		if parent := node.Parent(); parent != nil && parent.Type(language) == "statement_block" {
			for index := 0; index+1 < parent.NamedChildCount(); index++ {
				if parent.NamedChild(index).StartByte() == node.StartByte() {
					for next := index + 1; next < parent.NamedChildCount(); next++ {
						if sibling := parent.NamedChild(next); sibling.Type(language) != "comment" {
							alternative = sibling
							break
						}
					}

					break
				}
			}
		}
	}

	if alternative == nil {
		return nil
	}

	// Tree-sitter wraps the else body in an else_clause.
	if alternative.Type(language) == "else_clause" && alternative.NamedChildCount() > 0 {
		alternative = alternative.NamedChild(alternative.NamedChildCount() - 1)
	}

	rule, message := "", ""
	left := typescriptNodeTokens(consequence, language, file.source)
	right := typescriptNodeTokens(alternative, language, file.source)
	if consequence.Type(language) == "statement_block" && consequence.NamedChildCount() > 0 && left != `"{";"}";` && left == right {
		rule = "duplicate-branches"
		message = "conditional branches contain identical syntax; review consolidating them"
	}

	booleanReturn := func(branch *gotreesitter.Node) string {
		if branch.Type(language) == "statement_block" {
			var statement *gotreesitter.Node
			for index := 0; index < branch.NamedChildCount(); index++ {
				child := branch.NamedChild(index)
				if child.Type(language) == "comment" {
					continue
				}

				if statement != nil {
					return ""
				}

				statement = child
			}

			if statement == nil {
				return ""
			}

			branch = statement
		}

		if branch.Type(language) != "return_statement" || branch.NamedChildCount() != 1 {
			return ""
		}

		value := branch.NamedChild(0).Type(language)
		if value == "true" || value == "false" {
			return value
		}

		return ""
	}

	if first, second := booleanReturn(consequence), booleanReturn(alternative); first != "" && second != "" && first != second {
		condition := node.ChildByFieldName("condition", language)
		if condition != nil && condition.Type(language) == "parenthesized_expression" && condition.NamedChildCount() == 1 {
			condition = condition.NamedChild(0)
		}

		if condition != nil && condition.Type(language) == "binary_expression" {
			operator := condition.ChildByFieldName("operator", language)
			if operator != nil {
				switch operator.Text(file.source) {
				case "===", "!==", "==", "!=", "<", ">", "<=", ">=":
					rule = "redundant-boolean-return"
					message = "boolean branches return opposite literals; review replacing them with the condition or its negation"
				}
			}
		}
	}

	if rule == "" {
		return nil
	}

	content := typescriptNodeTokens(node, language, file.source)
	start, end := typescriptPosition(node, file.source)
	if alternative.EndByte() > node.EndByte() {
		_, end = typescriptPosition(alternative, file.source)
		content += typescriptNodeTokens(alternative, language, file.source)
	}

	return []report.Finding{{
		ID:          stableFindingID(file.path, rule, function.metric.ID, content, 0),
		Fingerprint: digestText("finding\x00" + function.metric.Fingerprint + "\x00" + content),
		Rule:        rule, Path: file.path, Severity: "review", Message: message, Start: start, End: end,
		IdentityAmbiguous: function.metric.IdentityAmbiguous,
		Details:           []report.Detail{{Key: "owner", Value: function.metric.Name}, {Key: "candidate_only", Value: "true"}},
	}}
}
