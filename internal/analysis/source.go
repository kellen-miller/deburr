package analysis

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"go/ast"
	"go/printer"
	"go/scanner"
	"go/token"
	"math"
	"sort"
	"strconv"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

type functionNode struct {
	node         ast.Node
	decl         *ast.FuncDecl
	lit          *ast.FuncLit
	parent       *functionNode
	name         string
	identityName string
	sourceDigest string
	ambiguous    bool
	start        int
	end          int
	startLine    int
	endLine      int
	startPos     report.Position
	endPos       report.Position
	sloc         int
	complexity   int
	nesting      int
	signals      structuralSignals
}

type structuralSignals struct {
	flatGuards         int
	fieldMappings      int
	nestedBranches     int
	assertionLikeCalls int
}

const anonymousAmbiguousMinimum = 2

func analyzeSource(file *sourceFile) ([]report.Function, []report.Finding) {
	nodes := collectFunctions(file)
	assignFunctionNames(nodes)
	assignFunctionLines(file, nodes)
	for _, function := range nodes {
		function.signals = structuralSignalsFor(file, function)
	}
	markAnonymousAmbiguity(nodes)

	functions := make([]report.Function, 0, len(nodes))
	for _, function := range nodes {
		id := functionID(file, function)
		functions = append(functions, report.Function{
			ID:             id,
			Fingerprint:    function.sourceDigest,
			Path:           file.path,
			Category:       file.category,
			Name:           function.name,
			Start:          function.startPos,
			End:            function.endPos,
			SLOC:           function.sloc,
			Cyclomatic:     function.complexity,
			MaxNesting:     function.nesting,
			Mass:           functionMass(function.complexity, function.sloc),
			HighComplexity: function.complexity > highComplexityThreshold,
			StructuralSignals: report.StructuralSignals{
				FlatGuards:         function.signals.flatGuards,
				FieldMappings:      function.signals.fieldMappings,
				NestedBranches:     function.signals.nestedBranches,
				AssertionLikeCalls: function.signals.assertionLikeCalls,
			},
			IdentityAmbiguous: function.ambiguous,
		})
	}

	findings := sourceFindings(file, nodes)
	return functions, findings
}

func markAnonymousAmbiguity(nodes []*functionNode) {
	byParent := make(map[*functionNode]map[string][]*functionNode)
	for _, function := range nodes {
		if function.decl != nil {
			continue
		}

		byDigest := byParent[function.parent]
		if byDigest == nil {
			byDigest = make(map[string][]*functionNode)
			byParent[function.parent] = byDigest
		}
		byDigest[function.sourceDigest] = append(byDigest[function.sourceDigest], function)
	}

	for _, byDigest := range byParent {
		for _, functions := range byDigest {
			if len(functions) < anonymousAmbiguousMinimum {
				continue
			}
			for _, function := range functions {
				function.ambiguous = true
			}
		}
	}
}

func collectFunctions(file *sourceFile) []*functionNode {
	nodes := make([]*functionNode, 0)
	ast.Inspect(file.astFile, func(node ast.Node) bool {
		switch typed := node.(type) {
		case *ast.FuncDecl:
			if typed.Body == nil {
				return true
			}

			nodes = append(nodes, makeFunctionNode(file, node, typed, nil))
		case *ast.FuncLit:
			nodes = append(nodes, makeFunctionNode(file, node, nil, typed))
		}

		return true
	})
	for _, function := range nodes {
		function.sourceDigest = normalizedNodeDigest(file, function.node)
	}
	sort.Slice(nodes, func(i, j int) bool {
		if nodes[i].start != nodes[j].start {
			return nodes[i].start < nodes[j].start
		}

		return nodes[i].end > nodes[j].end
	})
	for _, function := range nodes {
		for _, candidate := range nodes {
			if candidate == function || candidate.start > function.start || candidate.end < function.end {
				continue
			}

			if function.parent == nil || candidate.end-candidate.start < function.parent.end-function.parent.start {
				function.parent = candidate
			}
		}
	}

	return nodes
}

func makeFunctionNode(file *sourceFile, node ast.Node, decl *ast.FuncDecl, lit *ast.FuncLit) *functionNode {
	start := file.fileSet.PositionFor(node.Pos(), false)
	endOffset := node.End()
	if endOffset > node.Pos() {
		endOffset--
	}

	end := file.fileSet.PositionFor(endOffset, false)
	return &functionNode{
		node:      node,
		decl:      decl,
		lit:       lit,
		start:     int(node.Pos()),
		end:       int(node.End()),
		startLine: start.Line,
		endLine:   end.Line,
		startPos: report.Position{
			Line:   start.Line,
			Column: start.Column,
		},
		endPos: report.Position{
			Line:   end.Line,
			Column: end.Column,
		},
	}
}

func assignFunctionNames(nodes []*functionNode) {
	children := make(map[*functionNode][]*functionNode)
	roots := make([]*functionNode, 0)
	for _, function := range nodes {
		if function.parent == nil {
			roots = append(roots, function)
		} else {
			children[function.parent] = append(children[function.parent], function)
		}
	}

	sort.Slice(roots, func(i, j int) bool { return roots[i].start < roots[j].start })
	for index, function := range roots {
		function.name = declaredFunctionName(function)
		if function.name == "" {
			function.name = "file.func" + strconv.Itoa(index+1)
		}

		assignChildNames(function, children)
	}

	assignFunctionIdentities(roots, children)
}

func assignChildNames(parent *functionNode, children map[*functionNode][]*functionNode) {
	list := children[parent]
	sort.Slice(list, func(i, j int) bool { return list[i].start < list[j].start })
	for index, child := range list {
		child.name = parent.name + ".func" + strconv.Itoa(index+1)
		assignChildNames(child, children)
	}
}

func assignFunctionIdentities(roots []*functionNode, children map[*functionNode][]*functionNode) {
	counts := make(map[string]int)
	for _, function := range roots {
		counts[function.name]++
	}

	identityCounts := make(map[string]int)
	for _, function := range roots {
		function.ambiguous = counts[function.name] > 1
		assignFunctionIdentity(function, function.name, identityCounts, children)
	}
}

func assignFunctionIdentity(
	function *functionNode,
	base string,
	counts map[string]int,
	children map[*functionNode][]*functionNode,
) {
	occurrence := counts[base]
	counts[base] = occurrence + 1
	function.identityName = base
	if occurrence > 0 {
		function.identityName += "#" + strconv.Itoa(occurrence+1)
		function.ambiguous = true
	}
	if function.parent != nil && function.parent.ambiguous {
		function.ambiguous = true
	}

	childCounts := make(map[string]int)
	for _, child := range children[function] {
		childBase := function.identityName + ".anon-" + functionIdentityDigest(child)
		assignFunctionIdentity(child, childBase, childCounts, children)
	}
}

func functionIdentityDigest(function *functionNode) string {
	if function.sourceDigest != "" {
		return strings.TrimPrefix(function.sourceDigest, "sha256:")
	}

	return digestText("anonymous-function")
}

func declaredFunctionName(function *functionNode) string {
	if function.decl == nil || function.decl.Name == nil {
		return ""
	}

	name := function.decl.Name.Name
	if function.decl.Recv == nil || len(function.decl.Recv.List) == 0 {
		return name
	}

	return receiverTypeName(function.decl.Recv.List[0].Type) + "." + name
}

func receiverTypeName(expr ast.Expr) string {
	switch typed := expr.(type) {
	case *ast.Ident:
		return typed.Name
	case *ast.StarExpr:
		return "(*" + receiverTypeName(typed.X) + ")"
	case *ast.IndexExpr:
		return receiverTypeName(typed.X)
	case *ast.IndexListExpr:
		return receiverTypeName(typed.X)
	default:
		return "receiver"
	}
}

func assignFunctionLines(file *sourceFile, nodes []*functionNode) {
	for line := 1; line < len(file.lineCode); line++ {
		if !file.lineCode[line] {
			continue
		}

		var owner *functionNode
		for _, candidate := range nodes {
			if line < candidate.startLine || line > candidate.endLine {
				continue
			}

			if owner == nil || candidate.end-candidate.start < owner.end-owner.start {
				owner = candidate
			}
		}

		if owner != nil {
			owner.sloc++
		}
	}

	for _, function := range nodes {
		function.complexity, function.nesting = functionComplexity(function)
	}
}

func functionComplexity(function *functionNode) (int, int) {
	complexity := 1
	maxNesting := 0
	var walk func(ast.Node, int)
	walk = func(node ast.Node, depth int) {
		if node == nil {
			return
		}

		if node != function.node {
			switch node.(type) {
			case *ast.FuncDecl, *ast.FuncLit:
				return
			}
		}

		if isDecisionNode(node) {
			complexity += complexityContribution(node)
		}

		if isNestingControl(node) {
			depth++
			if depth > maxNesting {
				maxNesting = depth
			}
		}

		ast.Inspect(node, func(child ast.Node) bool {
			if child == node {
				return true
			}

			walk(child, depth)
			return false
		})
	}

	walk(functionBody(function), 0)
	return complexity, maxNesting
}

func functionBody(function *functionNode) ast.Node {
	if function.decl != nil {
		return function.decl.Body
	}

	return function.lit.Body
}

func isNestingControl(node ast.Node) bool {
	switch node.(type) {
	case *ast.IfStmt, *ast.ForStmt, *ast.RangeStmt, *ast.SwitchStmt,
		*ast.TypeSwitchStmt, *ast.SelectStmt:
		return true
	default:
		return false
	}
}

func isDecisionNode(node ast.Node) bool {
	if isNestingControl(node) {
		return true
	}

	binary, ok := node.(*ast.BinaryExpr)
	return ok && (binary.Op == token.LAND || binary.Op == token.LOR)
}

func complexityContribution(node ast.Node) int {
	contribution := 1
	switch typed := node.(type) {
	case *ast.BinaryExpr:
		if typed.Op != token.LAND && typed.Op != token.LOR {
			return 0
		}
	case *ast.SwitchStmt:
		contribution = switchCaseCount(typed.Body.List)
	case *ast.TypeSwitchStmt:
		contribution = switchCaseCount(typed.Body.List)
	case *ast.SelectStmt:
		contribution = selectCaseCount(typed.Body.List)
	}

	return contribution
}

func switchCaseCount(list []ast.Stmt) int {
	count := 0
	for _, statement := range list {
		clause, ok := statement.(*ast.CaseClause)
		if ok && clause.List != nil {
			count++
		}
	}

	return count
}

func selectCaseCount(list []ast.Stmt) int {
	count := 0
	for _, statement := range list {
		clause, ok := statement.(*ast.CommClause)
		if ok && clause.Comm != nil {
			count++
		}
	}

	return count
}

func functionMass(complexity, sloc int) float64 {
	if sloc <= 0 {
		return 0
	}

	return float64(complexity) * math.Sqrt(float64(sloc))
}

func functionID(file *sourceFile, function *functionNode) string {
	packageName := ""
	if file.astFile != nil && file.astFile.Name != nil {
		packageName = file.astFile.Name.Name
	}

	identityName := function.identityName
	if identityName == "" {
		identityName = function.name
	}

	return packageName + "/" + file.path + "::" + identityName
}

func functionIdentityName(function *functionNode) string {
	if function == nil {
		return ""
	}
	if function.identityName != "" {
		return function.identityName
	}
	return function.name
}

func normalizedNodeText(file *sourceFile, node ast.Node) string {
	var output bytes.Buffer
	if err := printer.Fprint(&output, file.fileSet, node); err != nil {
		return ""
	}

	return output.String()
}

func normalizedNodeDigest(file *sourceFile, node ast.Node) string {
	digest := sha256.Sum256([]byte(normalizedNodeTokens(file, node)))
	return fmt.Sprintf("sha256:%x", digest[:])
}

func normalizedNodeTokens(file *sourceFile, node ast.Node) string {
	if node == nil || file == nil || file.fileSet == nil {
		return ""
	}

	start := file.fileSet.PositionFor(node.Pos(), false).Offset
	end := file.fileSet.PositionFor(node.End(), false).Offset
	if start < 0 || end < start || end > len(file.source) {
		return ""
	}

	return normalizedSourceTokens(file.source[start:end])
}

func normalizedSourceTokens(source []byte) string {
	file := token.NewFileSet().AddFile("source.go", -1, len(source))
	var sourceScanner scanner.Scanner
	sourceScanner.Init(file, source, nil, scanner.ScanComments)

	var normalized strings.Builder
	for {
		_, tok, lit := sourceScanner.Scan()
		if tok == token.EOF {
			break
		}

		if tok == token.COMMENT {
			continue
		}

		normalized.WriteString(tok.String())
		normalized.WriteByte('=')
		normalized.WriteString(strconv.Quote(lit))
		normalized.WriteByte(';')
	}

	return normalized.String()
}

func stableFindingID(path, rule, owner, content string, occurrence int) string {
	digest := sha256.Sum256(
		[]byte(path + "\x00" + rule + "\x00" + owner + "\x00" + content + "\x00" + strconv.Itoa(occurrence)),
	)
	return fmt.Sprintf("%s-%x", rule, digest[:8])
}

func findingFingerprint(function *functionNode, content string) string {
	ownerDigest := ""
	if function != nil {
		ownerDigest = function.sourceDigest
	}

	return digestText("finding\x00" + ownerDigest + "\x00" + content)
}

func findingPosition(file *sourceFile, node ast.Node) (report.Position, report.Position) {
	start := file.fileSet.PositionFor(node.Pos(), false)
	endOffset := node.End()
	if endOffset > node.Pos() {
		endOffset--
	}

	end := file.fileSet.PositionFor(endOffset, false)
	return report.Position{Line: start.Line, Column: start.Column}, report.Position{Line: end.Line, Column: end.Column}
}
