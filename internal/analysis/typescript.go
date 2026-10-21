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

const (
	typescriptCommentNode        = "comment"
	typescriptStatementBlockNode = "statement_block"
	typescriptIterationBase      = 10_000
	typescriptStackDepthBase     = 1_000
	typescriptNodeLimitBase      = 300_000
	typescriptMemoryBudgetBase   = 512 * 1024 * 1024
	typescriptIterationPerByte   = 30
	typescriptStackPerByte       = 2
	typescriptNodesPerByte       = 52
	typescriptMemoryPerByte      = 512
)

type typescriptFunction struct {
	node   *gotreesitter.Node
	parent *typescriptFunction
	metric report.Function
}

func parseTypeScript(path string, source []byte) (*gotreesitter.Tree, *gotreesitter.Language, error) {
	language := typescript.Language()
	if filepath.Ext(path) == typeScriptReactExtension {
		language = tsx.Language()
	}

	parser := gotreesitter.NewParser(language)
	// Source-derived work limits make parse outcomes independent of host speed.
	parser.SetParseWorkLimits(gotreesitter.ParseWorkLimits{
		IterationLimit:  max(typescriptIterationBase, len(source)*typescriptIterationPerByte),
		StackDepthLimit: max(typescriptStackDepthBase, len(source)*typescriptStackPerByte),
		NodeLimit:       max(typescriptNodeLimitBase, len(source)*typescriptNodesPerByte),
	})
	parser.SetMemoryBudgetBytes(max(typescriptMemoryBudgetBase, int64(len(source))*typescriptMemoryPerByte))
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

func analyzeTypeScript(
	file *sourceFile,
	root *gotreesitter.Node,
	language *gotreesitter.Language,
) ([]report.Function, []report.Finding) {
	functions := collectTypeScriptFunctions(file, root, language)
	identityCounts := make(map[string]int, len(functions))
	for _, function := range functions {
		identityCounts[function.metric.ID]++
	}

	occurrences := make(map[string]int, len(functions))
	findings := make([]report.Finding, 0, len(functions))
	for _, function := range functions {
		assignTypeScriptIdentity(function, identityCounts, occurrences)
		findings = append(findings, analyzeTypeScriptFunction(file, function, language)...)
	}

	assignTypeScriptLines(file, functions)
	metrics := make([]report.Function, 0, len(functions))
	for _, function := range functions {
		function.metric.Mass = functionMass(function.metric.Cyclomatic, function.metric.SLOC)
		function.metric.HighComplexity = function.metric.Cyclomatic > highComplexityThreshold
		metrics = append(metrics, function.metric)
	}

	assignTypeScriptFindingIDs(findings)
	return metrics, findings
}

func collectTypeScriptFunctions(
	file *sourceFile,
	root *gotreesitter.Node,
	language *gotreesitter.Language,
) []*typescriptFunction {
	functions := make([]*typescriptFunction, 0)
	var collect func(*gotreesitter.Node, *typescriptFunction)
	collect = func(node *gotreesitter.Node, parent *typescriptFunction) {
		if function := typeScriptFunctionForNode(file, node, parent, language); function != nil {
			functions = append(functions, function)
			parent = function
		}

		childCount := node.NamedChildCount()
		for index := range childCount {
			collect(node.NamedChild(index), parent)
		}
	}

	collect(root, nil)
	return functions
}

func typeScriptFunctionForNode(
	file *sourceFile,
	node *gotreesitter.Node,
	parent *typescriptFunction,
	language *gotreesitter.Language,
) *typescriptFunction {
	if !typescriptFunctionNode(node, language) || node.ChildByFieldName("body", language) == nil {
		return nil
	}

	name := typescriptFunctionName(file, node, parent, language)
	fingerprint := "sha256:" + digestText(typescriptNodeTokens(node, language, file.source))
	if name == "" {
		name = "anon-" + strings.TrimPrefix(fingerprint, "sha256:")
	}

	start, end := typescriptPosition(node, file.source)
	return &typescriptFunction{
		node:   node,
		parent: parent,
		metric: report.Function{
			ID:          "typescript/" + file.path + "::" + name,
			Name:        name,
			Fingerprint: fingerprint,
			Path:        file.path,
			Category:    file.category,
			Start:       start,
			End:         end,
			Cyclomatic:  1,
		},
	}
}

func typescriptFunctionName(
	file *sourceFile,
	node *gotreesitter.Node,
	parent *typescriptFunction,
	language *gotreesitter.Language,
) string {
	name := ""
	if identifier := node.ChildByFieldName("name", language); identifier != nil {
		name = identifier.Text(file.source)
	} else if owner := node.Parent(); owner != nil {
		if ownerName := owner.ChildByFieldName("name", language); ownerName != nil {
			name = ownerName.Text(file.source)
		}
	}

	// Include classes between this function and its enclosing function.
	// Stopping there avoids repeating a method's class on nested arrows.
	for owner := node.Parent(); owner != nil; owner = owner.Parent() {
		if parent != nil && owner == parent.node {
			break
		}

		switch owner.Type(language) {
		case "class_declaration", "abstract_class_declaration", "class":
			if identifier := owner.ChildByFieldName("name", language); identifier != nil {
				name = identifier.Text(file.source) + "." + name
			}
		}
	}

	if parent != nil {
		name = parent.metric.Name + "." + name
	}

	return name
}

func assignTypeScriptIdentity(
	function *typescriptFunction,
	counts, occurrences map[string]int,
) {
	base := function.metric.ID
	function.metric.IdentityAmbiguous = counts[base] > 1 ||
		(function.parent != nil && function.parent.metric.IdentityAmbiguous)
	occurrences[base]++
	if occurrences[base] > 1 {
		function.metric.ID += "#" + strconv.Itoa(occurrences[base])
	}
}

func analyzeTypeScriptFunction(
	file *sourceFile,
	function *typescriptFunction,
	language *gotreesitter.Language,
) []report.Finding {
	findings := make([]report.Finding, 0)
	var walk func(*gotreesitter.Node, int)
	walk = func(node *gotreesitter.Node, depth int) {
		if node != function.node && typescriptFunctionNode(node, language) {
			return
		}

		depth = updateTypeScriptFunctionMetrics(function, node, depth, file.source, language)
		findings = append(findings, typescriptBranchFindings(file, function, node, language)...)
		childCount := node.NamedChildCount()
		for index := range childCount {
			walk(node.NamedChild(index), depth)
		}
	}

	walk(function.node.ChildByFieldName("body", language), 0)
	return findings
}

func updateTypeScriptFunctionMetrics(
	function *typescriptFunction,
	node *gotreesitter.Node,
	depth int,
	source []byte,
	language *gotreesitter.Language,
) int {
	switch node.Type(language) {
	case "if_statement",
		"for_statement",
		"for_in_statement",
		"while_statement",
		"do_statement",
		"catch_clause",
		"ternary_expression":
		function.metric.Cyclomatic++
		depth++
		if depth > 1 {
			function.metric.StructuralSignals.NestedBranches++
		}
	case "switch_statement":
		if depth > 0 {
			function.metric.StructuralSignals.NestedBranches++
		}

		depth++
	case "switch_case":
		function.metric.Cyclomatic++
	case "binary_expression":
		if operator := node.ChildByFieldName("operator", language); operator != nil {
			switch operator.Text(source) {
			case "&&", "||", "??":
				function.metric.Cyclomatic++
			}
		}
	}

	if depth > function.metric.MaxNesting {
		function.metric.MaxNesting = depth
	}

	return depth
}

func assignTypeScriptLines(file *sourceFile, functions []*typescriptFunction) {
	for line, code := range file.lineCode {
		if !code {
			continue
		}

		var owner *typescriptFunction
		for _, function := range functions {
			if line < function.metric.Start.Line || line > function.metric.End.Line {
				continue
			}

			if owner == nil ||
				function.node.EndByte()-function.node.StartByte() < owner.node.EndByte()-owner.node.StartByte() {
				owner = function
			}
		}

		if owner != nil {
			owner.metric.SLOC++
		}
	}
}

func assignTypeScriptFindingIDs(findings []report.Finding) {
	findingCounts := make(map[string]int, len(findings))
	for index := range findings {
		base := findings[index].ID
		findingCounts[base]++
		if findingCounts[base] > 1 {
			findings[index].ID += "#" + strconv.Itoa(findingCounts[base])
		}
	}
}

func typescriptFunctionNode(node *gotreesitter.Node, language *gotreesitter.Language) bool {
	switch node.Type(language) {
	case "function_declaration",
		"generator_function_declaration",
		"function_expression",
		"generator_function",
		"arrow_function",
		"method_definition":
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
	text       string
	start, end uint32
}

// Preserve literals verbatim; only syntax whitespace and comments are ignored.
func typescriptTokens(node *gotreesitter.Node, language *gotreesitter.Language, source []byte) []typescriptSourceToken {
	tokens := make([]typescriptSourceToken, 0)
	var walk func(*gotreesitter.Node)
	walk = func(current *gotreesitter.Node) {
		typeName := current.Type(language)
		if typeName == typescriptCommentNode {
			return
		}

		if current.ChildCount() == 0 || typeName == "string" || typeName == "template_string" || typeName == "regex" {
			tokens = append(tokens, typescriptSourceToken{
				start: current.StartByte(),
				end:   current.EndByte(),
				text:  current.Text(source),
			})
			return
		}

		childCount := current.ChildCount()
		for index := range childCount {
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

func typescriptLineMap(root *gotreesitter.Node, language *gotreesitter.Language, source []byte) []bool {
	lines := make([]bool, physicalLineCount(source)+1)
	var walk func(*gotreesitter.Node)
	walk = func(node *gotreesitter.Node) {
		if node.Type(language) == typescriptCommentNode {
			return
		}

		if node.ChildCount() == 0 {
			start, end := typescriptPosition(node, source)
			for line := start.Line; line <= end.Line && line < len(lines); line++ {
				lines[line] = true
			}

			return
		}

		childCount := node.ChildCount()
		for index := range childCount {
			walk(node.Child(index))
		}
	}

	walk(root)
	return lines
}
