package analysis

import (
	"go/ast"
	"go/token"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

func structuralSignalsFor(file *sourceFile, function *functionNode) structuralSignals {
	signals := structuralSignals{
		flatGuards:     flatGuardCount(functionBody(function)),
		fieldMappings:  fieldMappingCount(functionBody(function)),
		nestedBranches: nestedBranchCount(functionBody(function)),
	}
	if file.category == report.CategoryTest {
		signals.assertionLikeCalls = assertionLikeCallCount(functionBody(function))
	}

	return signals
}

func flatGuardCount(root ast.Node) int {
	body, ok := root.(*ast.BlockStmt)
	if !ok {
		return 0
	}

	count := 0
	for _, statement := range body.List {
		conditional, ok := statement.(*ast.IfStmt)
		if !ok || conditional.Else != nil || !guardBody(conditional.Body) {
			continue
		}

		count++
	}
	return count
}

func guardBody(body *ast.BlockStmt) bool {
	if body == nil || len(body.List) != 1 {
		return false
	}

	switch statement := body.List[0].(type) {
	case *ast.ReturnStmt:
		return true
	case *ast.BranchStmt:
		return statement.Tok == token.BREAK || statement.Tok == token.CONTINUE
	default:
		return false
	}
}

func fieldMappingCount(root ast.Node) int {
	count := 0
	walkFunctionNodes(root, func(node ast.Node) {
		literal, ok := node.(*ast.CompositeLit)
		if !ok || !fieldMappingLiteral(literal) {
			return
		}

		for _, element := range literal.Elts {
			keyValue, ok := element.(*ast.KeyValueExpr)
			if ok && isIdentifierLike(keyValue.Key) && isValueLike(keyValue.Value) {
				count++
			}
		}
	})
	return count
}

func fieldMappingLiteral(literal *ast.CompositeLit) bool {
	switch literal.Type.(type) {
	case *ast.Ident, *ast.SelectorExpr, *ast.StructType, *ast.StarExpr:
		return true
	default:
		return false
	}
}

func isIdentifierLike(node ast.Expr) bool {
	_, ok := node.(*ast.Ident)
	return ok
}

func isValueLike(node ast.Expr) bool {
	switch node.(type) {
	case *ast.Ident, *ast.SelectorExpr:
		return true
	default:
		return false
	}
}

func nestedBranchCount(root ast.Node) int {
	count := 0
	var walk func(ast.Node, int)
	walk = func(node ast.Node, depth int) {
		if node == nil {
			return
		}

		if node != root {
			switch node.(type) {
			case *ast.FuncDecl, *ast.FuncLit:
				return
			}
		}

		branchDepth := depth
		if isNestingControl(node) {
			if depth > 0 {
				count++
			}
			branchDepth++
		}

		ast.Inspect(node, func(child ast.Node) bool {
			if child == node {
				return true
			}

			walk(child, branchDepth)
			return false
		})
	}

	walk(root, 0)
	return count
}

func assertionLikeCallCount(root ast.Node) int {
	count := 0
	walkFunctionNodes(root, func(node ast.Node) {
		call, ok := node.(*ast.CallExpr)
		if ok && assertionLikeCall(call) {
			count++
		}
	})
	return count
}

func assertionLikeCall(call *ast.CallExpr) bool {
	if call == nil {
		return false
	}

	name := callName(call.Fun)
	if name == "" {
		return false
	}

	name = strings.ToLower(name)
	for _, prefix := range []string{"assert", "require", "expect"} {
		if strings.HasPrefix(name, prefix) {
			return true
		}
	}

	selector, ok := call.Fun.(*ast.SelectorExpr)
	if !ok {
		return false
	}

	packageName := selectorPackageName(selector.X)
	if packageName == "assert" || packageName == "require" || packageName == "testify" {
		return true
	}

	if !testingReceiverName(packageName) {
		return false
	}

	switch name {
	case "error", "errorf", "fatal", "fatalf", "fail", "failnow":
		return true
	default:
		return false
	}
}

func callName(expr ast.Expr) string {
	switch typed := expr.(type) {
	case *ast.Ident:
		return typed.Name
	case *ast.SelectorExpr:
		return typed.Sel.Name
	case *ast.IndexExpr:
		return callName(typed.X)
	case *ast.IndexListExpr:
		return callName(typed.X)
	default:
		return ""
	}
}

func selectorPackageName(expr ast.Expr) string {
	ident, ok := expr.(*ast.Ident)
	if !ok {
		return ""
	}

	return strings.ToLower(ident.Name)
}

func testingReceiverName(name string) bool {
	switch name {
	case "t", "tb", "tt", "test", "testingt":
		return true
	default:
		return false
	}
}
