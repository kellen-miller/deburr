package analysis

import (
	"github.com/kellen-miller/deburr/internal/report"
	"github.com/odvcencio/gotreesitter"
)

func typescriptBranchFindings(
	file *sourceFile,
	function *typescriptFunction,
	node *gotreesitter.Node,
	language *gotreesitter.Language,
) []report.Finding {
	if node.Type(language) != "if_statement" {
		return nil
	}

	consequence := node.ChildByFieldName("consequence", language)
	if consequence == nil {
		return nil
	}

	alternative := typescriptBranchAlternative(node, language)
	if alternative == nil {
		return nil
	}
	if alternative.Type(language) == "else_clause" && alternative.NamedChildCount() > 0 {
		alternative = alternative.NamedChild(alternative.NamedChildCount() - 1)
	}

	finding := typescriptRedundantBooleanReturnFinding(file, function, node, consequence, alternative, language)
	if finding != nil {
		return []report.Finding{*finding}
	}
	finding = typescriptDuplicateBranchFinding(file, function, node, consequence, alternative, language)
	if finding != nil {
		return []report.Finding{*finding}
	}

	return nil
}

func typescriptBranchAlternative(node *gotreesitter.Node, language *gotreesitter.Language) *gotreesitter.Node {
	if alternative := node.ChildByFieldName("alternative", language); alternative != nil {
		return alternative
	}

	parent := node.Parent()
	if parent == nil || parent.Type(language) != typescriptStatementBlockNode {
		return nil
	}

	childCount := parent.NamedChildCount()
	for index := range childCount {
		if index+1 >= childCount || parent.NamedChild(index).StartByte() != node.StartByte() {
			continue
		}

		for offset := range childCount - index - 1 {
			sibling := parent.NamedChild(index + 1 + offset)
			if sibling.Type(language) != typescriptCommentNode {
				return sibling
			}
		}

		return nil
	}

	return nil
}

func typescriptDuplicateBranchFinding(
	file *sourceFile,
	function *typescriptFunction,
	node, consequence, alternative *gotreesitter.Node,
	language *gotreesitter.Language,
) *report.Finding {
	left := typescriptNodeTokens(consequence, language, file.source)
	right := typescriptNodeTokens(alternative, language, file.source)
	if consequence.Type(language) != typescriptStatementBlockNode || consequence.NamedChildCount() == 0 ||
		left == `"{";"}";` || left != right {
		return nil
	}

	return typescriptConditionalFinding(
		file,
		function,
		node,
		alternative,
		language,
		ruleDuplicateBranches,
		"conditional branches contain identical syntax; review consolidating them",
	)
}

func typescriptRedundantBooleanReturnFinding(
	file *sourceFile,
	function *typescriptFunction,
	node, consequence, alternative *gotreesitter.Node,
	language *gotreesitter.Language,
) *report.Finding {
	first := typescriptBooleanReturn(consequence, language)
	second := typescriptBooleanReturn(alternative, language)
	if first == "" || second == "" || first == second {
		return nil
	}

	condition := node.ChildByFieldName("condition", language)
	if condition != nil && condition.Type(language) == "parenthesized_expression" && condition.NamedChildCount() == 1 {
		condition = condition.NamedChild(0)
	}
	if condition == nil || condition.Type(language) != "binary_expression" {
		return nil
	}

	operator := condition.ChildByFieldName("operator", language)
	if operator == nil || !typescriptComparisonOperator(operator.Text(file.source)) {
		return nil
	}

	return typescriptConditionalFinding(
		file,
		function,
		node,
		alternative,
		language,
		ruleRedundantBooleanReturn,
		"boolean branches return opposite literals; review replacing them with the condition or its negation",
	)
}

func typescriptBooleanReturn(branch *gotreesitter.Node, language *gotreesitter.Language) string {
	if branch.Type(language) == typescriptStatementBlockNode {
		var statement *gotreesitter.Node
		childCount := branch.NamedChildCount()
		for index := range childCount {
			child := branch.NamedChild(index)
			if child.Type(language) == typescriptCommentNode {
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
	if value == findingCandidateValue || value == "false" {
		return value
	}

	return ""
}

func typescriptComparisonOperator(operator string) bool {
	switch operator {
	case "===", "!==", "==", "!=", "<", ">", "<=", ">=":
		return true
	default:
		return false
	}
}

func typescriptConditionalFinding(
	file *sourceFile,
	function *typescriptFunction,
	node, alternative *gotreesitter.Node,
	language *gotreesitter.Language,
	rule, message string,
) *report.Finding {
	content := typescriptNodeTokens(node, language, file.source)
	start, end := typescriptPosition(node, file.source)
	if alternative.EndByte() > node.EndByte() {
		_, end = typescriptPosition(alternative, file.source)
		content += typescriptNodeTokens(alternative, language, file.source)
	}

	return &report.Finding{
		ID:                stableFindingID(file.path, rule, function.metric.ID, content, 0),
		Fingerprint:       digestText("finding\x00" + function.metric.Fingerprint + "\x00" + content),
		Rule:              rule,
		Path:              file.path,
		Severity:          findingSeverityReview,
		Message:           message,
		Start:             start,
		End:               end,
		IdentityAmbiguous: function.metric.IdentityAmbiguous,
		Details: []report.Detail{
			{Key: findingDetailOwner, Value: function.metric.Name},
			{Key: findingDetailCandidateOnly, Value: findingCandidateValue},
		},
	}
}
