package compare

import (
	"sort"

	"github.com/kellen-miller/deburr/internal/report"
)

func matchFindingIndexes(before, after []report.Finding) inventoryMatches {
	result := matchInventoryIndexes(
		findingIDsByValue(before),
		findingIDsByValue(after),
		findingFingerprintsByValue(before),
		findingFingerprintsByValue(after),
	)
	rejectAmbiguousMatches(&result, func(match inventoryMatch) bool {
		return before[match.before].IdentityAmbiguous || after[match.after].IdentityAmbiguous
	})
	return result
}

func matchFunctionIndexes(before, after []report.Function) inventoryMatches {
	result := matchInventoryIndexes(
		functionIDsByValue(before),
		functionIDsByValue(after),
		functionFingerprintsByValue(before),
		functionFingerprintsByValue(after),
	)
	rejectAmbiguousMatches(&result, func(match inventoryMatch) bool {
		return before[match.before].IdentityAmbiguous || after[match.after].IdentityAmbiguous
	})
	return result
}

func matchCloneIndexes(before, after []report.Clone) inventoryMatches {
	result := matchInventoryIndexes(
		cloneIDsByValue(before),
		cloneIDsByValue(after),
		cloneFingerprintsByValue(before),
		cloneFingerprintsByValue(after),
	)
	rejectAmbiguousMatches(&result, func(match inventoryMatch) bool {
		return before[match.before].IdentityAmbiguous || after[match.after].IdentityAmbiguous
	})
	return result
}

func rejectAmbiguousMatches(result *inventoryMatches, isAmbiguous func(inventoryMatch) bool) {
	kept := result.matches[:0]
	for _, match := range result.matches {
		if !isAmbiguous(match) {
			kept = append(kept, match)
			continue
		}

		delete(result.matchedBefore, match.before)
		delete(result.matchedAfter, match.after)
		result.blockedBefore[match.before] = true
		result.blockedAfter[match.after] = true
		result.ambiguities = append(result.ambiguities, inventoryAmbiguity{
			before: []int{match.before},
			after:  []int{match.after},
			basis:  match.basis,
		})
	}
	result.matches = kept
}

func matchInventoryIndexes(
	beforeIDs, afterIDs, beforeFingerprints, afterFingerprints map[string][]int,
) inventoryMatches {
	result := inventoryMatches{
		matches:       make([]inventoryMatch, 0),
		ambiguities:   make([]inventoryAmbiguity, 0),
		matchedBefore: make(map[int]bool),
		matchedAfter:  make(map[int]bool),
		blockedBefore: make(map[int]bool),
		blockedAfter:  make(map[int]bool),
	}
	matchInventoryKeys(&result, beforeIDs, afterIDs, MatchByID)
	beforeFingerprintIndexes := unmatchedFingerprintIndexes(
		beforeFingerprints,
		result.matchedBefore,
		result.blockedBefore,
	)
	afterFingerprintIndexes := unmatchedFingerprintIndexes(afterFingerprints, result.matchedAfter, result.blockedAfter)
	matchInventoryKeys(&result, beforeFingerprintIndexes, afterFingerprintIndexes, MatchByFingerprint)
	return result
}

func matchInventoryKeys(
	result *inventoryMatches,
	before, after map[string][]int,
	basis MatchBasis,
) {
	for _, key := range sortedMapKeys(before, after) {
		beforeIndexes := before[key]
		afterIndexes := after[key]
		switch {
		case len(beforeIndexes) == 1 && len(afterIndexes) == 1:
			result.matches = append(result.matches, inventoryMatch{
				before: beforeIndexes[0],
				after:  afterIndexes[0],
				basis:  basis,
			})
			result.matchedBefore[beforeIndexes[0]] = true
			result.matchedAfter[afterIndexes[0]] = true
		case len(beforeIndexes) > 1 || len(afterIndexes) > 1:
			result.ambiguities = append(result.ambiguities, inventoryAmbiguity{
				before: append([]int(nil), beforeIndexes...),
				after:  append([]int(nil), afterIndexes...),
				basis:  basis,
			})
			markInventoryIndexes(result.blockedBefore, beforeIndexes)
			markInventoryIndexes(result.blockedAfter, afterIndexes)
		}
	}
}

func findingIDsByValue(values []report.Finding) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].ID != "" {
			indexes[values[index].ID] = append(indexes[values[index].ID], index)
		}
	}
	return indexes
}

func functionIDsByValue(values []report.Function) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].ID != "" {
			indexes[values[index].ID] = append(indexes[values[index].ID], index)
		}
	}
	return indexes
}

func cloneIDsByValue(values []report.Clone) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].ID != "" {
			indexes[values[index].ID] = append(indexes[values[index].ID], index)
		}
	}
	return indexes
}

func findingFingerprintsByValue(values []report.Finding) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].Fingerprint != "" {
			indexes[values[index].Fingerprint] = append(indexes[values[index].Fingerprint], index)
		}
	}
	return indexes
}

func functionFingerprintsByValue(values []report.Function) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].Fingerprint != "" {
			indexes[values[index].Fingerprint] = append(indexes[values[index].Fingerprint], index)
		}
	}
	return indexes
}

func cloneFingerprintsByValue(values []report.Clone) map[string][]int {
	indexes := make(map[string][]int)
	for index := range values {
		if values[index].Fingerprint != "" {
			indexes[values[index].Fingerprint] = append(indexes[values[index].Fingerprint], index)
		}
	}
	return indexes
}

func unmatchedFingerprintIndexes(
	values map[string][]int,
	matched, blocked map[int]bool,
) map[string][]int {
	result := make(map[string][]int)
	for fingerprint, indexes := range values {
		for _, index := range indexes {
			if matched[index] || blocked[index] {
				continue
			}
			result[fingerprint] = append(result[fingerprint], index)
		}
	}
	return result
}

func sortedMapKeys(left, right map[string][]int) []string {
	keys := make(map[string]struct{}, len(left)+len(right))
	for key := range left {
		keys[key] = struct{}{}
	}
	for key := range right {
		keys[key] = struct{}{}
	}
	result := make([]string, 0, len(keys))
	for key := range keys {
		result = append(result, key)
	}
	sort.Strings(result)
	return result
}

func markInventoryIndexes(target map[int]bool, indexes []int) {
	for _, index := range indexes {
		target[index] = true
	}
}
