package report

import "testing"

func TestInitializeReviewIncludesCompleteInventory(t *testing.T) {
	value := reviewTestReport()
	ledger := InitializeReview(&value)

	if value.Review == nil {
		t.Fatal("InitializeReview did not attach the ledger")
	}

	if len(ledger.Items) != 4 {
		t.Fatalf("review item count = %d, want 4", len(ledger.Items))
	}

	if ledger.Counts != (ReviewCounts{Total: 4, Unreviewed: 4}) {
		t.Fatalf("review counts = %+v", ledger.Counts)
	}

	kinds := map[ReviewItemKind]int{}
	for index := range ledger.Items {
		item := ledger.Items[index]
		kinds[item.Kind]++
		if item.ID == "" || item.Fingerprint == "" || len(item.Locations) == 0 {
			t.Fatalf("incomplete review item = %+v", item)
		}
	}

	if kinds[ReviewFinding] != 1 || kinds[ReviewFunction] != 1 || kinds[ReviewClone] != 2 {
		t.Fatalf("review kinds = %+v", kinds)
	}
}

func TestReviewIdentitySurvivesLineMoves(t *testing.T) {
	before := reviewTestReport()
	after := reviewTestReport()
	after.Findings[0].Start.Line += 20
	after.Findings[0].End.Line += 20
	after.Functions[0].Start.Line += 20
	after.Functions[0].End.Line += 20
	for index := range after.Duplication.Clones {
		for locationIndex := range after.Duplication.Clones[index].Locations {
			after.Duplication.Clones[index].Locations[locationIndex].Start.Line += 20
			after.Duplication.Clones[index].Locations[locationIndex].End.Line += 20
		}
	}

	beforeLedger := InitializeReview(&before)
	afterLedger := InitializeReview(&after)
	if len(beforeLedger.Items) != len(afterLedger.Items) {
		t.Fatalf("line move changed inventory size: %d vs %d", len(beforeLedger.Items), len(afterLedger.Items))
	}

	for index := range beforeLedger.Items {
		left := beforeLedger.Items[index]
		right := afterLedger.Items[index]
		if left.ID != right.ID || left.Fingerprint != right.Fingerprint {
			t.Fatalf("line move changed identity: before=%+v after=%+v", left, right)
		}
	}
}

func TestApplyReviewAccountsDecisionsAndMissingItems(t *testing.T) {
	value := reviewTestReport()
	ledger := InitializeReview(&value)
	ledger.Items = ledger.Items[:1]
	decision := &ledger.Items[0]
	decision.Status = ReviewRetained
	decision.ReasonCategory = "semantics"
	decision.Reason = "behavior is intentional"
	decision.Evidence = "reviewed source"

	if err := ApplyReview(&value, &ledger); err != nil {
		t.Fatal(err)
	}

	if value.Review == nil {
		t.Fatal("ApplyReview did not attach the ledger")
	}

	if value.Review.Counts != (ReviewCounts{Total: 4, Unreviewed: 3, Retained: 1}) {
		t.Fatalf("review counts = %+v", value.Review.Counts)
	}

	for index := range value.Review.Items {
		item := value.Review.Items[index]
		if item.Status == ReviewRefactored && len(value.Findings) == 0 {
			t.Fatal("refactored review removed the native finding")
		}
	}
}

func TestApplyReviewMarksEditedFingerprintStale(t *testing.T) {
	value := reviewTestReport()
	ledger := InitializeReview(&value)
	for index := range ledger.Items {
		ledger.Items[index].Status = ReviewRefactored
		ledger.Items[index].ReasonCategory = "change"
		ledger.Items[index].Reason = "simplified"
		ledger.Items[index].Evidence = "reviewed source"
	}

	edited := reviewTestReport()
	edited.Findings[0].Message = "changed candidate"
	edited.Findings[0].Fingerprint = "finding-source-edited"
	if err := ApplyReview(&edited, &ledger); err != nil {
		t.Fatal(err)
	}

	stale := 0
	for index := range edited.Review.Items {
		item := edited.Review.Items[index]
		if !item.Stale {
			continue
		}

		stale++
		if item.Status != ReviewUnreviewed {
			t.Fatalf("stale item status = %q, want unreviewed", item.Status)
		}
		if item.Previous == nil || item.Previous.Status != ReviewRefactored ||
			item.Previous.Evidence == "" || item.Previous.Fingerprint == "" {
			t.Fatalf("stale item lost prior decision = %+v", item)
		}
	}

	if stale != 1 || edited.Review.Counts.Stale != 1 {
		t.Fatalf("stale accounting = %d/%d", stale, edited.Review.Counts.Stale)
	}
	if len(edited.Findings) != 1 {
		t.Fatal("ApplyReview changed native findings")
	}
}

func TestApplyReviewAllowsAmbiguousDecisionOnExactManifest(t *testing.T) {
	value := reviewTestReport()
	value.Provenance.Source.ManifestDigest = "sha256:source-a"
	value.Functions[0].IdentityAmbiguous = true
	ledger := InitializeReview(&value)
	functionDecision := reviewItem(&ledger, ReviewFunction)
	functionDecision.Status = ReviewRetained
	functionDecision.ReasonCategory = "semantics"
	functionDecision.Reason = "behavior is intentional"
	functionDecision.Evidence = "reviewed exact source snapshot"

	if err := ApplyReview(&value, &ledger); err != nil {
		t.Fatal(err)
	}

	applied := reviewItem(value.Review, ReviewFunction)
	if applied.Status != ReviewRetained || applied.Stale || applied.Previous != nil {
		t.Fatalf("exact ambiguous decision was not retained: %+v", applied)
	}
	if value.Review.SourceManifestDigest != "sha256:source-a" {
		t.Fatalf("applied manifest = %q", value.Review.SourceManifestDigest)
	}
}

func TestApplyReviewStalesAmbiguousDecisionOnManifestChange(t *testing.T) {
	baseline := reviewTestReport()
	baseline.Provenance.Source.ManifestDigest = "sha256:source-a"
	baseline.Functions[0].IdentityAmbiguous = true
	ledger := InitializeReview(&baseline)
	functionDecision := reviewItem(&ledger, ReviewFunction)
	functionDecision.Status = ReviewRetained
	functionDecision.ReasonCategory = "semantics"
	functionDecision.Reason = "behavior is intentional"
	functionDecision.Evidence = "reviewed baseline source"

	changed := reviewTestReport()
	changed.Provenance.Source.ManifestDigest = "sha256:source-b"
	changed.Functions[0].IdentityAmbiguous = true
	if err := ApplyReview(&changed, &ledger); err != nil {
		t.Fatal(err)
	}

	applied := reviewItem(changed.Review, ReviewFunction)
	if applied.Status != ReviewUnreviewed || !applied.Stale || applied.Previous == nil {
		t.Fatalf("changed ambiguous decision was not staled: %+v", applied)
	}
	if applied.Previous.Status != ReviewRetained || applied.Previous.Fingerprint == "" {
		t.Fatalf("stale ambiguous decision lost previous state: %+v", applied.Previous)
	}
	if changed.Review.SourceManifestDigest != "sha256:source-b" {
		t.Fatalf("changed applied manifest = %q", changed.Review.SourceManifestDigest)
	}
}

func TestApplyReviewStalesAmbiguousDecisionWithUnknownManifest(t *testing.T) {
	baseline := reviewTestReport()
	baseline.Provenance.Source.ManifestDigest = "sha256:source-a"
	baseline.Functions[0].IdentityAmbiguous = true
	ledger := InitializeReview(&baseline)
	functionDecision := reviewItem(&ledger, ReviewFunction)
	functionDecision.Status = ReviewRetained
	functionDecision.ReasonCategory = "semantics"
	functionDecision.Reason = "behavior is intentional"
	functionDecision.Evidence = "reviewed baseline source"

	unknown := reviewTestReport()
	unknown.Functions[0].IdentityAmbiguous = true
	if err := ApplyReview(&unknown, &ledger); err != nil {
		t.Fatal(err)
	}

	applied := reviewItem(unknown.Review, ReviewFunction)
	if applied.Status != ReviewUnreviewed || !applied.Stale || applied.Previous == nil {
		t.Fatalf("unknown ambiguous decision was not staled: %+v", applied)
	}
	if unknown.Review.SourceManifestDigest != "" {
		t.Fatalf("unknown applied manifest = %q", unknown.Review.SourceManifestDigest)
	}
}

func TestApplyReviewAllowsNamedDecisionAcrossManifestChange(t *testing.T) {
	baseline := reviewTestReport()
	baseline.Provenance.Source.ManifestDigest = "sha256:source-a"
	ledger := InitializeReview(&baseline)
	functionDecision := reviewItem(&ledger, ReviewFunction)
	functionDecision.Status = ReviewRetained
	functionDecision.ReasonCategory = "semantics"
	functionDecision.Reason = "behavior is intentional"
	functionDecision.Evidence = "reviewed named function"

	changed := reviewTestReport()
	changed.Provenance.Source.ManifestDigest = "sha256:source-b"
	if err := ApplyReview(&changed, &ledger); err != nil {
		t.Fatal(err)
	}

	applied := reviewItem(changed.Review, ReviewFunction)
	if applied.Status != ReviewRetained || applied.Stale || applied.Previous != nil {
		t.Fatalf("named decision changed with manifest: %+v", applied)
	}
}

func TestInitializeReviewPropagatesIdentityAmbiguity(t *testing.T) {
	value := reviewTestReport()
	value.Findings[0].IdentityAmbiguous = true
	value.Functions[0].IdentityAmbiguous = true
	value.Duplication.Clones[0].IdentityAmbiguous = true
	ledger := InitializeReview(&value)

	if !reviewItem(&ledger, ReviewFinding).IdentityAmbiguous ||
		!reviewItem(&ledger, ReviewFunction).IdentityAmbiguous {
		t.Fatalf("native ambiguity was not propagated: %+v", ledger.Items)
	}
	cloneItems := 0
	for index := range ledger.Items {
		if ledger.Items[index].Kind == ReviewClone && ledger.Items[index].IdentityAmbiguous {
			cloneItems++
		}
	}
	if cloneItems != 1 {
		t.Fatalf("clone ambiguity count = %d, want one", cloneItems)
	}
}

func TestApplyReviewRejectsUnknownDuplicateAndInvalidDecisions(t *testing.T) {
	tests := []struct {
		mutate func(*ReviewLedger)
		name   string
	}{
		{
			name: "unknown",
			mutate: func(ledger *ReviewLedger) {
				ledger.Items[0].ID = "unknown"
			},
		},
		{
			name: "duplicate",
			mutate: func(ledger *ReviewLedger) {
				ledger.Items = append(ledger.Items, ledger.Items[0])
			},
		},
		{
			name: "retained reason",
			mutate: func(ledger *ReviewLedger) {
				ledger.Items[0].Status = ReviewRetained
			},
		},
		{
			name: "upstream deferral",
			mutate: func(ledger *ReviewLedger) {
				ledger.Items[0].Status = ReviewDeferred
				ledger.Items[0].ReasonCategory = ReviewReasonCategoryUpstream
				ledger.Items[0].Reason = "blocked"
				ledger.Items[0].Evidence = "waiting for owner"
			},
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			value := reviewTestReport()
			ledger := InitializeReview(&value)
			test.mutate(&ledger)
			if err := ApplyReview(&value, &ledger); err == nil {
				t.Fatal("ApplyReview succeeded")
			}
		})
	}
}

func TestApplyReviewRequiresMatchingSchema(t *testing.T) {
	value := reviewTestReport()
	ledger := InitializeReview(&value)
	ledger.SchemaVersion = "1"
	if err := ApplyReview(&value, &ledger); err == nil {
		t.Fatal("ApplyReview accepted a mismatched ledger schema")
	}

	ledger = InitializeReview(&value)
	value.Config.Language = "other"
	if err := ApplyReview(&value, &ledger); err == nil {
		t.Fatal("ApplyReview accepted a mismatched report identity")
	}
}

func TestApplyReviewRejectsAmbiguousNativeIDs(t *testing.T) {
	value := reviewTestReport()
	value.Findings = append(value.Findings, value.Findings[0])
	ledger := InitializeReview(&value)
	if err := ApplyReview(&value, &ledger); err == nil {
		t.Fatal("ApplyReview accepted duplicate native IDs")
	}
}

func reviewItem(ledger *ReviewLedger, kind ReviewItemKind) *ReviewItem {
	for index := range ledger.Items {
		if ledger.Items[index].Kind == kind {
			return &ledger.Items[index]
		}
	}

	return nil
}

func reviewTestReport() Report {
	return Report{
		SchemaVersion: SchemaVersion,
		Findings: []Finding{{
			ID:          "finding-native",
			Fingerprint: "finding-source-1",
			Rule:        "rule",
			Path:        "main.go",
			Severity:    "review",
			Message:     "candidate",
			Details:     []Detail{{Key: "owner", Value: "Main"}},
			Start:       Position{Line: 4, Column: 2},
			End:         Position{Line: 6, Column: 2},
		}},
		Functions: []Function{
			{
				ID:             "function-native",
				Fingerprint:    "function-source-1",
				Path:           "main.go",
				Category:       CategoryProduction,
				Name:           "Main",
				Start:          Position{Line: 10, Column: 1},
				End:            Position{Line: 40, Column: 1},
				SLOC:           30,
				Cyclomatic:     11,
				MaxNesting:     2,
				Mass:           60,
				HighComplexity: true,
			},
			{Path: "main.go", Name: "Small"},
		},
		Duplication: &Duplication{
			Status: DuplicationMeasured,
			Clones: []Clone{
				{
					ID:          "clone-native-1",
					Fingerprint: "clone-source-1",
					Category:    CategoryProduction,
					Tokens:      80,
					Lines:       8,
					Locations: []Location{{
						Path:  "main.go",
						Start: Position{Line: 50, Column: 1},
						End:   Position{Line: 57, Column: 2},
					}, {
						Path:  "other.go",
						Start: Position{Line: 12, Column: 1},
						End:   Position{Line: 19, Column: 2},
					}},
				},
				{
					ID:          "clone-native-2",
					Fingerprint: "clone-source-2",
					Category:    CategoryTest,
					Tokens:      90,
					Lines:       9,
					Locations: []Location{{
						Path:  "main_test.go",
						Start: Position{Line: 20, Column: 1},
						End:   Position{Line: 28, Column: 2},
					}, {
						Path:  "other_test.go",
						Start: Position{Line: 30, Column: 1},
						End:   Position{Line: 38, Column: 2},
					}},
				},
			},
		},
	}
}
