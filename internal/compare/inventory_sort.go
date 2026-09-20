package compare

import (
	"fmt"
	"sort"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

func functionIDs(values []report.Function, indexes []int) []string {
	ids := make([]string, 0, len(indexes))
	for _, index := range indexes {
		ids = append(ids, values[index].ID)
	}
	sort.Strings(ids)
	return ids
}

func cloneIDs(values []report.Clone, indexes []int) []string {
	ids := make([]string, 0, len(indexes))
	for _, index := range indexes {
		ids = append(ids, values[index].ID)
	}
	sort.Strings(ids)
	return ids
}

func sortFunctions(values []report.Function) {
	sort.SliceStable(values, func(left, right int) bool {
		return functionSortKey(&values[left]) < functionSortKey(&values[right])
	})
}

func sortClones(values []report.Clone) {
	sort.SliceStable(values, func(left, right int) bool {
		return cloneSortKey(&values[left]) < cloneSortKey(&values[right])
	})
}

func functionSortKey(value *report.Function) string {
	return fmt.Sprintf(
		"%s:%09d:%09d:%s:%s",
		value.Path,
		value.Start.Line,
		value.Start.Column,
		value.Name,
		value.ID,
	)
}

func cloneSortKey(value *report.Clone) string {
	path := clonePath(value)
	return fmt.Sprintf("%s:%s:%09d:%09d:%s", value.Category, path, value.Tokens, value.Lines, value.ID)
}

func clonePath(value *report.Clone) string {
	if value == nil || len(value.Locations) == 0 {
		return ""
	}
	location := value.Locations[0]
	return fmt.Sprintf(
		"%s:%09d:%09d:%09d:%09d",
		location.Path,
		location.Start.Line,
		location.Start.Column,
		location.End.Line,
		location.End.Column,
	)
}

func sortFindingChanges(values []FindingComparison) {
	sort.SliceStable(values, func(left, right int) bool {
		return findingChangeSortKey(&values[left]) < findingChangeSortKey(&values[right])
	})
}

func findingChangeSortKey(value *FindingComparison) string {
	path, line, column, id := "", 0, 0, ""
	if value.Before != nil {
		path, line, column, id = value.Before.Path, value.Before.Start.Line, value.Before.Start.Column, value.Before.ID
	}
	if value.After != nil {
		path, line, column, id = value.After.Path, value.After.Start.Line, value.After.Start.Column, value.After.ID
	}
	return comparisonChangeSortKey(path, line, column, value.State, id)
}

func sortFunctionChanges(values []FunctionComparison) {
	sort.SliceStable(values, func(left, right int) bool {
		return functionChangeSortKey(&values[left]) < functionChangeSortKey(&values[right])
	})
}

func functionChangeSortKey(value *FunctionComparison) string {
	path, line, column, id := "", 0, 0, ""
	if value.Before != nil {
		path, line, column, id = value.Before.Path, value.Before.Start.Line, value.Before.Start.Column, value.Before.ID
	}
	if value.After != nil {
		path, line, column, id = value.After.Path, value.After.Start.Line, value.After.Start.Column, value.After.ID
	}
	return comparisonChangeSortKey(path, line, column, value.State, id)
}

func comparisonChangeSortKey(
	path string,
	line, column int,
	state ComparisonState,
	id string,
) string {
	return fmt.Sprintf("%s:%09d:%09d:%s:%s", path, line, column, state, id)
}

func sortCloneChanges(values []CloneComparison) {
	sort.SliceStable(values, func(left, right int) bool {
		return cloneChangeSortKey(&values[left]) < cloneChangeSortKey(&values[right])
	})
}

func cloneChangeSortKey(value *CloneComparison) string {
	path, id := "", ""
	if value.Before != nil {
		path, id = clonePath(value.Before), value.Before.ID
	}
	if value.After != nil {
		path, id = clonePath(value.After), value.After.ID
	}
	return strings.Join([]string{path, string(value.State), id}, ":")
}

func sortAmbiguous(values []AmbiguousMatch) {
	sort.SliceStable(values, func(left, right int) bool {
		leftKey := strings.Join(
			[]string{values[left].Kind, values[left].Path, values[left].Rule, values[left].Message},
			":",
		)
		rightKey := strings.Join(
			[]string{values[right].Kind, values[right].Path, values[right].Rule, values[right].Message},
			":",
		)
		return leftKey < rightKey
	})
}
