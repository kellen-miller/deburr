package analysis

import (
	"go/ast"
	"go/token"
	"strconv"

	"github.com/kellen-miller/deburr/internal/report"
)

const (
	findingCandidateValue = "true"
	findingRulesPerNode   = 2
)

func sourceFindings(file *sourceFile, nodes []*functionNode) []report.Finding {
	findings := make([]report.Finding, 0, len(nodes)*findingRulesPerNode)
	for _, function := range nodes {
		findings = append(findings, redundantBooleanFindings(file, function)...)
		findings = append(findings, duplicateBranchFindings(file, function)...)
	}

	return findings
}

func redundantBooleanFindings(file *sourceFile, function *functionNode) []report.Finding {
	if !returnsPlainBool(function) {
		return nil
	}

	findings := make([]report.Finding, 0)
	occurrence := 0
	walkFunctionBlocks(functionBody(function), func(block *ast.BlockStmt) {
		for index := 0; index+1 < len(block.List); index++ {
			conditional, ok := block.List[index].(*ast.IfStmt)
			if !ok || conditional.Init != nil || conditional.Else != nil || len(conditional.Body.List) != 1 {
				continue
			}

			thenReturn, ok := conditional.Body.List[0].(*ast.ReturnStmt)
			if !ok || len(thenReturn.Results) != 1 {
				continue
			}

			following, ok := block.List[index+1].(*ast.ReturnStmt)
			if !ok || len(following.Results) != 1 {
				continue
			}

			thenValue, thenOK := booleanLiteral(thenReturn.Results[0])
			followingValue, followingOK := booleanLiteral(following.Results[0])
			if !thenOK || !followingOK || !comparisonExpr(conditional.Cond) || thenValue == followingValue {
				continue
			}

			occurrence++
			start, _ := findingPosition(file, conditional)
			_, end := findingPosition(file, following)
			content := normalizedNodeText(file, conditional) + normalizedNodeText(file, following)
			findings = append(findings, report.Finding{
				ID: stableFindingID(
					file.path,
					"redundant-boolean-return",
					functionIdentityName(function),
					content,
					occurrence,
				),
				Fingerprint:       findingFingerprint(function, content),
				Rule:              "redundant-boolean-return",
				Path:              file.path,
				Start:             start,
				End:               end,
				Severity:          "review",
				Message:           "boolean branches return opposite literals; review replacing them with the condition or its negation",
				IdentityAmbiguous: function.ambiguous,
				Details: []report.Detail{
					{Key: "owner", Value: function.name},
					{Key: "condition", Value: normalizedNodeText(file, conditional.Cond)},
					{Key: "then_return", Value: strconv.FormatBool(thenValue)},
					{Key: "else_return", Value: strconv.FormatBool(followingValue)},
					{Key: "candidate_only", Value: findingCandidateValue},
				},
			})
		}
	})
	return findings
}

func duplicateBranchFindings(file *sourceFile, function *functionNode) []report.Finding {
	findings := make([]report.Finding, 0)
	occurrence := 0
	walkFunctionNodes(functionBody(function), func(node ast.Node) {
		conditional, ok := node.(*ast.IfStmt)
		if !ok {
			return
		}

		thenBody := conditional.Body
		elseBody, ok := conditional.Else.(*ast.BlockStmt)
		if !ok || len(thenBody.List) == 0 || len(elseBody.List) == 0 {
			return
		}

		if normalizedNodeText(file, thenBody) != normalizedNodeText(file, elseBody) {
			return
		}

		occurrence++
		start, end := findingPosition(file, conditional)
		content := normalizedNodeText(file, thenBody)
		findings = append(findings, report.Finding{
			ID: stableFindingID(
				file.path,
				"duplicate-branches",
				functionIdentityName(function),
				content,
				occurrence,
			),
			Fingerprint:       findingFingerprint(function, content),
			Rule:              "duplicate-branches",
			Path:              file.path,
			Start:             start,
			End:               end,
			Severity:          "review",
			Message:           "if branches have identical syntax; review whether the condition and branch effects can be simplified",
			IdentityAmbiguous: function.ambiguous,
			Details: []report.Detail{
				{Key: "owner", Value: function.name},
				{Key: "candidate_only", Value: findingCandidateValue},
				{Key: "preserve_condition", Value: findingCandidateValue},
			},
		})
	})
	return findings
}

func returnsPlainBool(function *functionNode) bool {
	if function.decl == nil || function.decl.Type == nil || function.decl.Type.Results == nil ||
		len(function.decl.Type.Results.List) != 1 {
		return false
	}

	result := function.decl.Type.Results.List[0]
	return len(result.Names) == 0 && result.Type != nil && result.Type.Pos() != token.NoPos && isPlainBool(result.Type)
}

func isPlainBool(expr ast.Expr) bool {
	ident, ok := expr.(*ast.Ident)
	return ok && ident.Name == "bool"
}

func booleanLiteral(expr ast.Expr) (bool, bool) {
	ident, ok := expr.(*ast.Ident)
	if !ok {
		return false, false
	}

	if ident.Name == "true" {
		return true, true
	}

	if ident.Name == "false" {
		return false, true
	}

	return false, false
}

func comparisonExpr(expr ast.Expr) bool {
	binary, ok := expr.(*ast.BinaryExpr)
	if !ok {
		return false
	}

	switch binary.Op {
	case token.EQL, token.NEQ, token.LSS, token.GTR, token.LEQ, token.GEQ:
		return true
	default:
		return false
	}
}

func walkFunctionBlocks(node ast.Node, visit func(*ast.BlockStmt)) {
	if node == nil {
		return
	}

	ast.Inspect(node, func(child ast.Node) bool {
		if child != node {
			switch child.(type) {
			case *ast.FuncDecl, *ast.FuncLit:
				return false
			}
		}

		if block, ok := child.(*ast.BlockStmt); ok {
			visit(block)
		}

		return true
	})
}

func walkFunctionNodes(node ast.Node, visit func(ast.Node)) {
	if node == nil {
		return
	}

	ast.Inspect(node, func(child ast.Node) bool {
		if child != node {
			switch child.(type) {
			case *ast.FuncDecl, *ast.FuncLit:
				return false
			}
		}

		visit(child)
		return true
	})
}
