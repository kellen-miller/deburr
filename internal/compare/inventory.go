package compare

import (
	"fmt"

	"github.com/kellen-miller/deburr/internal/report"
)

type inventoryMatch struct {
	basis  MatchBasis
	before int
	after  int
}

type inventoryAmbiguity struct {
	basis  MatchBasis
	before []int
	after  []int
}

type inventoryMatches struct {
	matchedBefore map[int]bool
	matchedAfter  map[int]bool
	blockedBefore map[int]bool
	blockedAfter  map[int]bool
	matches       []inventoryMatch
	ambiguities   []inventoryAmbiguity
}

func reviewLedgerForCompare(value *report.Report) (report.ReviewLedger, error) {
	prepared := *value
	if value.Review == nil {
		return report.InitializeReview(&prepared), nil
	}

	ledger := *value.Review
	prepared.Review = nil
	if err := report.ApplyReview(&prepared, &ledger); err != nil {
		return report.ReviewLedger{}, fmt.Errorf("apply review ledger: %w", err)
	}

	return *prepared.Review, nil
}

func compareFindings(
	beforeReport, afterReport *report.Report,
	beforeReview, afterReview *report.ReviewLedger,
) ([]FindingComparison, []AmbiguousMatch) {
	before := append([]report.Finding(nil), beforeReport.Findings...)
	after := append([]report.Finding(nil), afterReport.Findings...)
	sortFindings(before)
	sortFindings(after)

	matched := matchFindingIndexes(before, after)
	beforeLookup := reviewLookup(beforeReview, report.ReviewFinding)
	afterLookup := reviewLookup(afterReview, report.ReviewFinding)
	changes := make([]FindingComparison, 0, len(before)+len(after))
	for _, pair := range matched.matches {
		changes = append(changes, FindingComparison{
			State:        ComparisonMatched,
			MatchBasis:   pair.basis,
			Before:       &before[pair.before],
			After:        &after[pair.after],
			BeforeReview: beforeLookup[before[pair.before].ID],
			AfterReview:  afterLookup[after[pair.after].ID],
		})
	}

	ambiguous := make([]AmbiguousMatch, 0, len(matched.ambiguities))
	for _, group := range matched.ambiguities {
		change := FindingComparison{
			State:      ComparisonAmbiguous,
			MatchBasis: group.basis,
			BeforeIDs:  findingIDs(before, group.before),
			AfterIDs:   findingIDs(after, group.after),
		}
		if len(group.before) > 0 {
			change.Before = &before[group.before[0]]
			change.BeforeReview = beforeLookup[before[group.before[0]].ID]
		}
		if len(group.after) > 0 {
			change.After = &after[group.after[0]]
			change.AfterReview = afterLookup[after[group.after[0]].ID]
		}
		changes = append(changes, change)
		ambiguous = append(ambiguous, findingAmbiguousMatch(before, after, group))
	}

	for index := range before {
		if matched.matchedBefore[index] || matched.blockedBefore[index] {
			continue
		}

		changes = append(changes, FindingComparison{
			State:        findingRemovedState(&before[index], afterReport),
			Before:       &before[index],
			BeforeReview: beforeLookup[before[index].ID],
		})
	}
	for index := range after {
		if matched.matchedAfter[index] || matched.blockedAfter[index] {
			continue
		}

		changes = append(changes, FindingComparison{
			State:       ComparisonNew,
			After:       &after[index],
			AfterReview: afterLookup[after[index].ID],
		})
	}

	sortFindingChanges(changes)
	sortAmbiguous(ambiguous)
	return changes, ambiguous
}

func compareFunctions(
	beforeReport, afterReport *report.Report,
	beforeReview, afterReview *report.ReviewLedger,
) ([]FunctionComparison, []AmbiguousMatch) {
	before := append([]report.Function(nil), beforeReport.Functions...)
	after := append([]report.Function(nil), afterReport.Functions...)
	sortFunctions(before)
	sortFunctions(after)

	matched := matchFunctionIndexes(before, after)
	beforeLookup := reviewLookup(beforeReview, report.ReviewFunction)
	afterLookup := reviewLookup(afterReview, report.ReviewFunction)
	changes := make([]FunctionComparison, 0, len(before)+len(after))
	for _, pair := range matched.matches {
		beforeFunction := &before[pair.before]
		afterFunction := &after[pair.after]
		changes = append(changes, FunctionComparison{
			State:        functionMatchState(beforeFunction, afterFunction),
			MatchBasis:   pair.basis,
			Before:       beforeFunction,
			After:        afterFunction,
			BeforeReview: beforeLookup[beforeFunction.ID],
			AfterReview:  afterLookup[afterFunction.ID],
		})
	}

	ambiguous := make([]AmbiguousMatch, 0, len(matched.ambiguities))
	for _, group := range matched.ambiguities {
		change := FunctionComparison{
			State:      ComparisonAmbiguous,
			MatchBasis: group.basis,
			BeforeIDs:  functionIDs(before, group.before),
			AfterIDs:   functionIDs(after, group.after),
		}
		if len(group.before) > 0 {
			change.Before = &before[group.before[0]]
			change.BeforeReview = beforeLookup[before[group.before[0]].ID]
		}
		if len(group.after) > 0 {
			change.After = &after[group.after[0]]
			change.AfterReview = afterLookup[after[group.after[0]].ID]
		}
		changes = append(changes, change)
		ambiguous = append(ambiguous, functionAmbiguousMatch(before, after, group))
	}

	for index := range before {
		if matched.matchedBefore[index] || matched.blockedBefore[index] {
			continue
		}

		changes = append(changes, FunctionComparison{
			State:        functionRemovedState(&before[index], afterReport),
			Before:       &before[index],
			BeforeReview: beforeLookup[before[index].ID],
		})
	}
	for index := range after {
		if matched.matchedAfter[index] || matched.blockedAfter[index] {
			continue
		}

		changes = append(changes, FunctionComparison{
			State:       ComparisonNew,
			After:       &after[index],
			AfterReview: afterLookup[after[index].ID],
		})
	}

	sortFunctionChanges(changes)
	sortAmbiguous(ambiguous)
	return changes, ambiguous
}

func compareClones(
	beforeReport, afterReport *report.Report,
	beforeReview, afterReview *report.ReviewLedger,
) ([]CloneComparison, []AmbiguousMatch) {
	before := reportClones(beforeReport.Duplication)
	after := reportClones(afterReport.Duplication)
	sortClones(before)
	sortClones(after)

	matched := matchCloneIndexes(before, after)
	beforeLookup := reviewLookup(beforeReview, report.ReviewClone)
	afterLookup := reviewLookup(afterReview, report.ReviewClone)
	changes := make([]CloneComparison, 0, len(before)+len(after))
	for _, pair := range matched.matches {
		beforeClone := &before[pair.before]
		afterClone := &after[pair.after]
		changes = append(changes, CloneComparison{
			State:        ComparisonMatched,
			MatchBasis:   pair.basis,
			Before:       beforeClone,
			After:        afterClone,
			BeforeReview: beforeLookup[beforeClone.ID],
			AfterReview:  afterLookup[afterClone.ID],
		})
	}

	ambiguous := make([]AmbiguousMatch, 0, len(matched.ambiguities))
	for _, group := range matched.ambiguities {
		change := CloneComparison{
			State:      ComparisonAmbiguous,
			MatchBasis: group.basis,
			BeforeIDs:  cloneIDs(before, group.before),
			AfterIDs:   cloneIDs(after, group.after),
		}
		if len(group.before) > 0 {
			change.Before = &before[group.before[0]]
			change.BeforeReview = beforeLookup[before[group.before[0]].ID]
		}
		if len(group.after) > 0 {
			change.After = &after[group.after[0]]
			change.AfterReview = afterLookup[after[group.after[0]].ID]
		}
		changes = append(changes, change)
		ambiguous = append(ambiguous, cloneAmbiguousMatch(before, after, group))
	}

	for index := range before {
		if matched.matchedBefore[index] || matched.blockedBefore[index] {
			continue
		}

		changes = append(changes, CloneComparison{
			State:        ComparisonRemoved,
			Before:       &before[index],
			BeforeReview: beforeLookup[before[index].ID],
		})
	}
	for index := range after {
		if matched.matchedAfter[index] || matched.blockedAfter[index] {
			continue
		}

		changes = append(changes, CloneComparison{
			State:       ComparisonNew,
			After:       &after[index],
			AfterReview: afterLookup[after[index].ID],
		})
	}

	sortCloneChanges(changes)
	sortAmbiguous(ambiguous)
	return changes, ambiguous
}

func findingAggregates(changes []FindingComparison) ([]report.Finding, []report.Finding) {
	newFindings := make([]report.Finding, 0)
	noLongerReported := make([]report.Finding, 0)
	for index := range changes {
		change := &changes[index]
		switch change.State {
		case ComparisonNew:
			if change.After != nil {
				newFindings = append(newFindings, *change.After)
			}
		case ComparisonNoLongerReported, ComparisonSourceRemoved, ComparisonRemoved:
			if change.Before != nil {
				noLongerReported = append(noLongerReported, *change.Before)
			}
		default:
			continue
		}
	}
	sortFindings(newFindings)
	sortFindings(noLongerReported)
	return newFindings, noLongerReported
}

func findingRemovedState(finding *report.Finding, after *report.Report) ComparisonState {
	if sourcePathKnown(after) && !sourcePathPresent(after, finding.Path) {
		return ComparisonSourceRemoved
	}
	return ComparisonNoLongerReported
}

func functionRemovedState(function *report.Function, after *report.Report) ComparisonState {
	if sourcePathKnown(after) && !sourcePathPresent(after, function.Path) {
		return ComparisonSourceRemoved
	}
	return ComparisonDeletedFunction
}

func sourcePathKnown(value *report.Report) bool {
	return len(value.Files) > 0
}

func sourcePathPresent(value *report.Report, path string) bool {
	for index := range value.Files {
		if value.Files[index].Path == path && value.Files[index].Status == report.FileAnalyzed {
			return true
		}
	}
	for index := range value.Functions {
		if value.Functions[index].Path == path {
			return true
		}
	}
	for index := range value.Findings {
		if value.Findings[index].Path == path {
			return true
		}
	}
	return false
}

func functionMatchState(before, after *report.Function) ComparisonState {
	switch {
	case before.HighComplexity && !after.HighComplexity:
		return ComparisonResolvedThreshold
	case !before.HighComplexity && after.HighComplexity:
		return ComparisonCrossedThreshold
	default:
		return ComparisonMatched
	}
}

func reviewLookup(ledger *report.ReviewLedger, kind report.ReviewItemKind) map[string]*report.ReviewItem {
	lookup := make(map[string]*report.ReviewItem)
	if ledger == nil {
		return lookup
	}
	for index := range ledger.Items {
		item := ledger.Items[index]
		if item.Kind != kind || item.ID == "" {
			continue
		}
		copyItem := item
		lookup[item.ID] = &copyItem
	}
	return lookup
}

func findingAmbiguousMatch(
	before []report.Finding,
	after []report.Finding,
	group inventoryAmbiguity,
) AmbiguousMatch {
	match := AmbiguousMatch{
		Kind:       string(report.ReviewFinding),
		State:      ComparisonAmbiguous,
		MatchBasis: group.basis,
		BeforeIDs:  findingIDs(before, group.before),
		AfterIDs:   findingIDs(after, group.after),
	}
	if len(group.before) > 0 {
		value := before[group.before[0]]
		match.Rule, match.Path, match.Message = value.Rule, value.Path, value.Message
	} else if len(group.after) > 0 {
		value := after[group.after[0]]
		match.Rule, match.Path, match.Message = value.Rule, value.Path, value.Message
	}
	return match
}

func functionAmbiguousMatch(
	before []report.Function,
	after []report.Function,
	group inventoryAmbiguity,
) AmbiguousMatch {
	match := AmbiguousMatch{
		Kind:       string(report.ReviewFunction),
		State:      ComparisonAmbiguous,
		MatchBasis: group.basis,
		BeforeIDs:  functionIDs(before, group.before),
		AfterIDs:   functionIDs(after, group.after),
	}
	if len(group.before) > 0 {
		value := before[group.before[0]]
		match.Path, match.Message = value.Path, value.Name
	} else if len(group.after) > 0 {
		value := after[group.after[0]]
		match.Path, match.Message = value.Path, value.Name
	}
	return match
}

func cloneAmbiguousMatch(
	before []report.Clone,
	after []report.Clone,
	group inventoryAmbiguity,
) AmbiguousMatch {
	match := AmbiguousMatch{
		Kind:       string(report.ReviewClone),
		State:      ComparisonAmbiguous,
		MatchBasis: group.basis,
		BeforeIDs:  cloneIDs(before, group.before),
		AfterIDs:   cloneIDs(after, group.after),
	}
	if len(group.before) > 0 {
		value := before[group.before[0]]
		match.Path, match.Message = clonePath(&value), "duplicate source spans"
	} else if len(group.after) > 0 {
		value := after[group.after[0]]
		match.Path, match.Message = clonePath(&value), "duplicate source spans"
	}
	return match
}

func reportClones(value *report.Duplication) []report.Clone {
	if value == nil {
		return nil
	}
	return append([]report.Clone(nil), value.Clones...)
}
