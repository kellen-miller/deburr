package report

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
)

type ReviewStatus string

const (
	ReviewUnreviewed ReviewStatus = "unreviewed"
	ReviewRefactored ReviewStatus = "refactored"
	ReviewRetained   ReviewStatus = "retained"
	ReviewDeferred   ReviewStatus = "deferred"
)

type ReviewItemKind string

const (
	ReviewFinding  ReviewItemKind = "finding"
	ReviewFunction ReviewItemKind = "function"
	ReviewClone    ReviewItemKind = "clone"
)

const ReviewReasonCategoryUpstream = "upstream"

type ReviewLedger struct {
	SchemaVersion        string       `json:"schema_version"`
	ReportIdentity       string       `json:"report_identity"`
	SourceManifestDigest string       `json:"source_manifest_digest"`
	Items                []ReviewItem `json:"items"`
	Counts               ReviewCounts `json:"counts"`
}

type ReviewCounts struct {
	Total      int `json:"total"`
	Unreviewed int `json:"unreviewed"`
	Refactored int `json:"refactored"`
	Retained   int `json:"retained"`
	Deferred   int `json:"deferred"`
	Stale      int `json:"stale"`
}

type ReviewDecision struct {
	Fingerprint    string       `json:"fingerprint,omitempty"`
	Status         ReviewStatus `json:"status"`
	ReasonCategory string       `json:"reason_category,omitempty"`
	Reason         string       `json:"reason,omitempty"`
	Evidence       string       `json:"evidence,omitempty"`
	Upstream       string       `json:"upstream,omitempty"`
}

type ReviewItem struct {
	Previous          *ReviewDecision `json:"previous,omitempty"`
	ID                string          `json:"id"`
	Kind              ReviewItemKind  `json:"kind"`
	Fingerprint       string          `json:"fingerprint"`
	Status            ReviewStatus    `json:"status"`
	ReasonCategory    string          `json:"reason_category,omitempty"`
	Reason            string          `json:"reason,omitempty"`
	Evidence          string          `json:"evidence,omitempty"`
	Upstream          string          `json:"upstream,omitempty"`
	Locations         []Location      `json:"locations"`
	Stale             bool            `json:"stale"`
	IdentityAmbiguous bool            `json:"identity_ambiguous"`
}

// InitializeReview creates the complete review inventory and attaches it to
// value. It includes every finding, high-complexity function, and clone.
func InitializeReview(value *Report) ReviewLedger {
	ledger := ReviewLedger{SchemaVersion: SchemaVersion, Items: []ReviewItem{}}
	if value == nil {
		return ledger
	}

	ledger.SchemaVersion = value.SchemaVersion
	if ledger.SchemaVersion == "" {
		ledger.SchemaVersion = SchemaVersion
	}

	ledger.ReportIdentity = reportIdentity(value)
	ledger.SourceManifestDigest = value.Provenance.Source.ManifestDigest
	ledger.Items = reviewInventory(value)
	finishReviewLedger(&ledger)
	value.Review = &ledger
	return ledger
}

// ApplyReview validates and applies decisions from an external ledger. Items
// missing from the ledger remain unreviewed. Changed fingerprints remain in
// the inventory with stale=true and their prior decision preserved.
func ApplyReview(value *Report, ledger *ReviewLedger) error {
	identity, err := validateReviewEnvelope(value, ledger)
	if err != nil {
		return err
	}

	current, currentByID, err := reviewInventoryIndex(value)
	if err != nil {
		return err
	}

	if err := applyReviewDecisions(
		current,
		currentByID,
		ledger,
		value.Provenance.Source.ManifestDigest,
	); err != nil {
		return err
	}

	result := ReviewLedger{
		SchemaVersion:        value.SchemaVersion,
		ReportIdentity:       identity,
		SourceManifestDigest: value.Provenance.Source.ManifestDigest,
		Items:                current,
	}
	finishReviewLedger(&result)
	value.Review = &result
	return nil
}

func validateReviewEnvelope(value *Report, ledger *ReviewLedger) (string, error) {
	if value == nil {
		return "", errors.New("report is required")
	}
	if ledger == nil {
		return "", errors.New("review ledger is required")
	}

	if value.SchemaVersion == "" {
		return "", errors.New("report schema_version is required")
	}

	if value.SchemaVersion != SchemaVersion {
		return "", fmt.Errorf("unsupported report schema_version %q", value.SchemaVersion)
	}

	if ledger.SchemaVersion == "" {
		return "", errors.New("review schema_version is required")
	}

	if ledger.SchemaVersion != value.SchemaVersion {
		return "", fmt.Errorf(
			"review schema_version %q does not match report schema_version %q",
			ledger.SchemaVersion,
			value.SchemaVersion,
		)
	}

	identity := reportIdentity(value)
	if ledger.ReportIdentity == "" {
		return "", errors.New("review report_identity is required")
	}

	if ledger.ReportIdentity != identity {
		return "", errors.New("review report_identity does not match report")
	}

	return identity, nil
}

func reviewInventoryIndex(value *Report) ([]ReviewItem, map[string]int, error) {
	current := reviewInventory(value)
	currentByID := make(map[string]int, len(current))
	for index := range current {
		item := &current[index]
		if item.ID == "" {
			return nil, nil, fmt.Errorf("review inventory item %d has no id", index)
		}

		if item.Fingerprint == "" {
			return nil, nil, fmt.Errorf("review inventory item %q has no fingerprint", item.ID)
		}

		if _, exists := currentByID[item.ID]; exists {
			return nil, nil, fmt.Errorf("review inventory contains duplicate id %q", item.ID)
		}

		currentByID[item.ID] = index
	}

	return current, currentByID, nil
}

func applyReviewDecisions(
	current []ReviewItem,
	currentByID map[string]int,
	ledger *ReviewLedger,
	manifest string,
) error {
	decisions := append([]ReviewItem(nil), ledger.Items...)
	sort.SliceStable(decisions, func(left, right int) bool {
		return decisions[left].ID < decisions[right].ID
	})
	seen := make(map[string]struct{}, len(decisions))
	for index := range decisions {
		decision := &decisions[index]
		currentIndex, err := validateReviewItemBinding(decision, current, currentByID, seen)
		if err != nil {
			return err
		}

		item := &current[currentIndex]
		if decision.Fingerprint != item.Fingerprint || decision.Stale ||
			!reviewDecisionAllowed(item, decision, ledger.SourceManifestDigest, manifest) {
			markReviewStale(item, decision)
			continue
		}

		copyReviewDecision(item, decision)
	}

	return nil
}

func validateReviewItemBinding(
	decision *ReviewItem,
	current []ReviewItem,
	currentByID map[string]int,
	seen map[string]struct{},
) (int, error) {
	if decision.ID == "" {
		return 0, errors.New("review item id is required")
	}

	if _, exists := seen[decision.ID]; exists {
		return 0, fmt.Errorf("review ledger contains duplicate id %q", decision.ID)
	}
	seen[decision.ID] = struct{}{}

	currentIndex, exists := currentByID[decision.ID]
	if !exists {
		return 0, fmt.Errorf("review ledger contains unknown id %q", decision.ID)
	}

	item := &current[currentIndex]
	if decision.Kind != item.Kind {
		return 0, fmt.Errorf(
			"review item %q kind %q does not match current kind %q",
			decision.ID,
			decision.Kind,
			item.Kind,
		)
	}

	if err := validateReviewDecision(decision); err != nil {
		return 0, fmt.Errorf("review item %q: %w", decision.ID, err)
	}

	if decision.Fingerprint == "" {
		return 0, fmt.Errorf("review item %q fingerprint is required", decision.ID)
	}

	return currentIndex, nil
}

func reviewDecisionAllowed(
	item *ReviewItem,
	decision *ReviewItem,
	ledgerManifest, reportManifest string,
) bool {
	if !item.IdentityAmbiguous && !decision.IdentityAmbiguous {
		return true
	}
	if decision.Status == ReviewUnreviewed {
		return true
	}

	return ledgerManifest != "" && ledgerManifest == reportManifest
}

func markReviewStale(target, decision *ReviewItem) {
	target.Stale = true
	target.Status = ReviewUnreviewed
	target.Previous = previousDecision(decision)
}

func reviewInventory(value *Report) []ReviewItem {
	items := make([]ReviewItem, 0, len(value.Findings)+len(value.Functions)+reviewCloneCount(value))
	for index := range value.Findings {
		finding := value.Findings[index]
		items = append(items, ReviewItem{
			ID:                finding.ID,
			Kind:              ReviewFinding,
			Fingerprint:       finding.Fingerprint,
			Status:            ReviewUnreviewed,
			IdentityAmbiguous: finding.IdentityAmbiguous,
			Locations:         []Location{{Path: finding.Path, Start: finding.Start, End: finding.End}},
		})
	}

	for index := range value.Functions {
		function := value.Functions[index]
		if !function.HighComplexity {
			continue
		}

		items = append(items, ReviewItem{
			ID:                function.ID,
			Kind:              ReviewFunction,
			Fingerprint:       function.Fingerprint,
			Status:            ReviewUnreviewed,
			IdentityAmbiguous: function.IdentityAmbiguous,
			Locations:         []Location{{Path: function.Path, Start: function.Start, End: function.End}},
		})
	}

	if value.Duplication != nil {
		for index := range value.Duplication.Clones {
			clone := value.Duplication.Clones[index]
			items = append(items, ReviewItem{
				ID:                clone.ID,
				Kind:              ReviewClone,
				Fingerprint:       clone.Fingerprint,
				Status:            ReviewUnreviewed,
				IdentityAmbiguous: clone.IdentityAmbiguous,
				Locations:         append([]Location(nil), clone.Locations...),
			})
		}
	}

	sort.SliceStable(items, func(left, right int) bool {
		if items[left].Kind != items[right].Kind {
			return items[left].Kind < items[right].Kind
		}

		return items[left].ID < items[right].ID
	})
	return items
}

func reviewCloneCount(value *Report) int {
	if value.Duplication == nil {
		return 0
	}

	return len(value.Duplication.Clones)
}

func finishReviewLedger(ledger *ReviewLedger) {
	counts := ReviewCounts{Total: len(ledger.Items)}
	for index := range ledger.Items {
		switch ledger.Items[index].Status {
		case ReviewUnreviewed:
			counts.Unreviewed++
		case ReviewRefactored:
			counts.Refactored++
		case ReviewRetained:
			counts.Retained++
		case ReviewDeferred:
			counts.Deferred++
		}

		if ledger.Items[index].Stale {
			counts.Stale++
		}
	}

	ledger.Counts = counts
}

func validateReviewDecision(item *ReviewItem) error {
	switch item.Status {
	case ReviewUnreviewed:
		return nil
	case ReviewRefactored, ReviewRetained, ReviewDeferred:
		if strings.TrimSpace(item.ReasonCategory) == "" {
			return errors.New("reason_category is required for a decided item")
		}

		if strings.TrimSpace(item.Reason) == "" {
			return errors.New("reason is required for a decided item")
		}

		if strings.TrimSpace(item.Evidence) == "" {
			return errors.New("evidence is required for a decided item")
		}

		if item.Status == ReviewDeferred && item.ReasonCategory == ReviewReasonCategoryUpstream &&
			strings.TrimSpace(item.Upstream) == "" {
			return errors.New("upstream is required for an upstream deferral")
		}

		return nil
	default:
		return fmt.Errorf("unsupported status %q", item.Status)
	}
}

func copyReviewDecision(target, source *ReviewItem) {
	target.Status = source.Status
	target.ReasonCategory = source.ReasonCategory
	target.Reason = source.Reason
	target.Evidence = source.Evidence
	target.Upstream = source.Upstream
	target.Previous = nil
}

func previousDecision(item *ReviewItem) *ReviewDecision {
	if item.Previous != nil {
		previous := *item.Previous
		return &previous
	}

	if item.Status == ReviewUnreviewed && strings.TrimSpace(item.ReasonCategory) == "" &&
		strings.TrimSpace(item.Reason) == "" && strings.TrimSpace(item.Evidence) == "" &&
		strings.TrimSpace(item.Upstream) == "" {
		return nil
	}

	return &ReviewDecision{
		Fingerprint:    item.Fingerprint,
		Status:         item.Status,
		ReasonCategory: item.ReasonCategory,
		Reason:         item.Reason,
		Evidence:       item.Evidence,
		Upstream:       item.Upstream,
	}
}

func reportIdentity(value *Report) string {
	identity, err := json.Marshal(struct {
		Analyzer      AnalyzerIdentity `json:"analyzer"`
		SchemaVersion string           `json:"schema_version"`
		Scope         Scope            `json:"scope"`
		Config        ConfigSummary    `json:"config"`
	}{
		Analyzer:      value.Analyzer,
		SchemaVersion: value.SchemaVersion,
		Scope:         value.Scope,
		Config:        value.Config,
	})
	if err != nil {
		return ""
	}

	digest := sha256.Sum256(identity)
	return "sha256:" + hex.EncodeToString(digest[:])
}
