package render

const comparisonHTMLPage = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Deburr comparison</title>
<style>
body { font: 15px system-ui, sans-serif; margin: 2rem; color: #202124; }
table { border-collapse: collapse; margin: 1rem 0 2rem; width: 100%; }
th, td { border: 1px solid #dadce0; padding: .4rem .6rem; text-align: left; vertical-align: top; }
th { background: #f1f3f4; }
code { white-space: pre-wrap; overflow-wrap: anywhere; }
.bad { color: #b00020; }
</style>
</head>
<body>
<h1>Deburr comparison</h1>
<p>Schema <code>{{.SchemaVersion}}</code>; compatible <code>{{.Compatible}}</code>; complete <code>{{.Complete}}</code>{{with .Reason}}; reason <code>{{.}}</code>{{end}}.</p>
<p>Before provenance <code>{{provenanceText .BeforeProvenance}}</code>.</p>
<p>After provenance <code>{{provenanceText .AfterProvenance}}</code>.</p>
{{if not .Compatible}}<p class="bad">Change conclusion unavailable; no finding, function, or clone improvement claim was made.</p>{{else if not .Complete}}<p class="bad">Change conclusion incomplete; no finding, function, or clone improvement claim was made.</p>{{end}}
<h2>Coverage</h2>
<table><tr><th></th><th>Analyzed</th><th>Read errors</th><th>Parse errors</th><th>Unsupported</th><th>Excluded</th></tr>
<tr><th>Before</th><td>{{.BeforeCoverage.Analyzed}}</td><td>{{.BeforeCoverage.ReadErrors}}</td><td>{{.BeforeCoverage.ParseErrors}}</td><td>{{.BeforeCoverage.Unsupported}}</td><td>{{.BeforeCoverage.Excluded}}</td></tr>
<tr><th>After</th><td>{{.AfterCoverage.Analyzed}}</td><td>{{.AfterCoverage.ReadErrors}}</td><td>{{.AfterCoverage.ParseErrors}}</td><td>{{.AfterCoverage.Unsupported}}</td><td>{{.AfterCoverage.Excluded}}</td></tr></table>
<h2>Raw metrics</h2>
<table><tr><th>Bucket</th><th>Code lines</th><th>Functions</th><th>Cyclomatic</th><th>Max nesting</th><th>Mass</th><th>High complexity functions</th><th>High complexity mass</th></tr>
<tr><th>Before all</th><td>{{.BeforeMetrics.All.CodeLines}}</td><td>{{.BeforeMetrics.All.Functions}}</td><td>{{.BeforeMetrics.All.Cyclomatic}}</td><td>{{.BeforeMetrics.All.MaxNesting}}</td><td>{{printf "%.6f" .BeforeMetrics.All.Mass}}</td><td>{{.BeforeMetrics.All.HighComplexityFunctions}}</td><td>{{printf "%.6f" .BeforeMetrics.All.HighComplexityMass}}</td></tr>
<tr><th>After all</th><td>{{.AfterMetrics.All.CodeLines}}</td><td>{{.AfterMetrics.All.Functions}}</td><td>{{.AfterMetrics.All.Cyclomatic}}</td><td>{{.AfterMetrics.All.MaxNesting}}</td><td>{{printf "%.6f" .AfterMetrics.All.Mass}}</td><td>{{.AfterMetrics.All.HighComplexityFunctions}}</td><td>{{printf "%.6f" .AfterMetrics.All.HighComplexityMass}}</td></tr>
{{with .MetricsDelta}}<tr><th>Delta all</th><td>{{.All.CodeLines}}</td><td>{{.All.Functions}}</td><td>{{.All.Cyclomatic}}</td><td>{{.All.MaxNesting}}</td><td>{{printf "%+.6f" .All.Mass}}</td><td>{{.All.HighComplexityFunctions}}</td><td>{{printf "%+.6f" .All.HighComplexityMass}}</td></tr>{{else}}<tr><th colspan="8" class="bad">Metrics delta unavailable.</th></tr>{{end}}
</table>
<h2>New findings ({{len .NewFindings}})</h2>
<table><tr><th>Location</th><th>Rule</th><th>Severity</th><th>Message</th><th>ID</th><th>Identity</th></tr>
{{range .NewFindings}}<tr><td><code>{{.Path}}:{{.Start.Line}}:{{.Start.Column}}-{{.End.Line}}:{{.End.Column}}</code></td><td>{{.Rule}}</td><td>{{.Severity}}</td><td>{{.Message}}</td><td><code>{{.ID}}</code></td><td>{{.IdentityAmbiguous}}</td></tr>{{else}}<tr><td colspan="6">No new findings.</td></tr>{{end}}
</table>
<h2>No longer reported findings ({{len .ResolvedFindings}})</h2>
<table><tr><th>Location</th><th>Rule</th><th>Severity</th><th>Message</th><th>ID</th><th>Identity</th></tr>
{{range .ResolvedFindings}}<tr><td><code>{{.Path}}:{{.Start.Line}}:{{.Start.Column}}-{{.End.Line}}:{{.End.Column}}</code></td><td>{{.Rule}}</td><td>{{.Severity}}</td><td>{{.Message}}</td><td><code>{{.ID}}</code></td><td>{{.IdentityAmbiguous}}</td></tr>{{else}}<tr><td colspan="6">No findings are no longer reported.</td></tr>{{end}}
</table>
<h2>Review ledger</h2>
<p>Before: total={{.BeforeReview.Counts.Total}}, unreviewed={{.BeforeReview.Counts.Unreviewed}}, refactored={{.BeforeReview.Counts.Refactored}}, retained={{.BeforeReview.Counts.Retained}}, deferred={{.BeforeReview.Counts.Deferred}}, stale={{.BeforeReview.Counts.Stale}}.</p>
<p>After: total={{.AfterReview.Counts.Total}}, unreviewed={{.AfterReview.Counts.Unreviewed}}, refactored={{.AfterReview.Counts.Refactored}}, retained={{.AfterReview.Counts.Retained}}, deferred={{.AfterReview.Counts.Deferred}}, stale={{.AfterReview.Counts.Stale}}.</p>
<h2>Finding changes ({{len .FindingChanges}})</h2>
<table><tr><th>State</th><th>Match</th><th>Before</th><th>After</th><th>Review</th></tr>
{{range .FindingChanges}}<tr><td>{{.State}}</td><td>{{.MatchBasis}}<br><code>{{comparisonIDs .BeforeIDs}}</code><code>{{comparisonIDs .AfterIDs}}</code></td><td><code>{{findingComparisonText .Before}}</code></td><td><code>{{findingComparisonText .After}}</code></td><td>{{reviewComparisonText .BeforeReview .AfterReview}}</td></tr>{{else}}<tr><td colspan="5">No finding changes.</td></tr>{{end}}
</table>
<h2>Function changes ({{len .FunctionChanges}})</h2>
<table><tr><th>State</th><th>Match</th><th>Before measurements and location</th><th>After measurements and location</th><th>Review</th></tr>
{{range .FunctionChanges}}<tr><td>{{.State}}{{if or (eq .State "resolved_threshold") (eq .State "deleted_function") (eq .State "source_removed")}}<br><small>no behavior conclusion</small>{{end}}</td><td>{{.MatchBasis}}<br><code>{{comparisonIDs .BeforeIDs}}</code><code>{{comparisonIDs .AfterIDs}}</code></td><td><code>{{functionComparisonText .Before}}</code></td><td><code>{{functionComparisonText .After}}</code></td><td>{{reviewComparisonText .BeforeReview .AfterReview}}</td></tr>{{else}}<tr><td colspan="5">No function changes.</td></tr>{{end}}
</table>
<h2>Clone changes ({{len .CloneChanges}})</h2>
<table><tr><th>State</th><th>Match</th><th>Before family, measurements, and locations</th><th>After family, measurements, and locations</th><th>Review</th></tr>
{{range .CloneChanges}}<tr><td>{{.State}}</td><td>{{.MatchBasis}}<br><code>{{comparisonIDs .BeforeIDs}}</code><code>{{comparisonIDs .AfterIDs}}</code></td><td><code>{{cloneComparisonText .Before}}</code></td><td><code>{{cloneComparisonText .After}}</code></td><td>{{reviewComparisonText .BeforeReview .AfterReview}}</td></tr>{{else}}<tr><td colspan="5">No clone changes.</td></tr>{{end}}
</table>
<h2>Duplication</h2>
<p>Before <code>{{.Duplication.BeforeStatus}}</code>; after <code>{{.Duplication.AfterStatus}}</code>; comparable <code>{{.Duplication.Comparable}}</code>{{with .Duplication.Reason}}; reason <code>{{.}}</code>{{end}}.</p>
{{if .Duplication.BeforeClones}}<h3>Before clones</h3><ul>{{range .Duplication.BeforeClones}}<li><code>{{.ID}}</code>: {{.Tokens}} tokens, {{.Lines}} lines</li>{{end}}</ul>{{end}}
{{if .Duplication.AfterClones}}<h3>After clones</h3><ul>{{range .Duplication.AfterClones}}<li><code>{{.ID}}</code>: {{.Tokens}} tokens, {{.Lines}} lines</li>{{end}}</ul>{{end}}
</body>
</html>
`

const htmlPage = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Deburr report</title>
<style>
body { font: 15px system-ui, sans-serif; margin: 2rem; color: #202124; }
table { border-collapse: collapse; margin: 1rem 0 2rem; width: 100%; }
th, td { border: 1px solid #dadce0; padding: .4rem .6rem; text-align: left; vertical-align: top; }
th { background: #f1f3f4; }
code { white-space: pre-wrap; overflow-wrap: anywhere; }
.error { color: #b00020; }
.filters { display: flex; flex-wrap: wrap; gap: .75rem; align-items: end; }
.filters label { display: flex; flex-direction: column; gap: .2rem; }
.filters input, .filters select { min-width: 10rem; padding: .35rem; }
[hidden] { display: none; }
</style>
</head>
<body>
<h1>Deburr report</h1>
<p>Schema <code>{{.SchemaVersion}}</code>; analyzer <code>{{.Analyzer.ID}} {{.Analyzer.Version}}</code>; language <code>{{.Config.Language}}</code>.</p>
<p>Provenance <code>{{provenanceText .Provenance}}</code>.</p>
<h2>Coverage</h2>
<table><tr><th>Discovered</th><th>Analyzed</th><th>Excluded</th><th>Unsupported</th><th>Read errors</th><th>Parse errors</th><th>Symlinks</th><th>Excluded directories</th></tr>
<tr><td>{{.Coverage.Discovered}}</td><td>{{.Coverage.Analyzed}}</td><td>{{.Coverage.Excluded}}</td><td>{{.Coverage.Unsupported}}</td><td>{{.Coverage.ReadErrors}}</td><td>{{.Coverage.ParseErrors}}</td><td>{{.Coverage.Symlinks}}</td><td>{{.Coverage.ExcludedDirectories}}</td></tr></table>
{{if eq .Coverage.Analyzed 0}}<p class="error">No analyzable Go files were found.</p>{{end}}
<h2>Metrics</h2>
<table><tr><th>Bucket</th><th>Code lines</th><th>Functions</th><th>Cyclomatic</th><th>Max nesting</th><th>Mass</th><th>High complexity functions</th><th>High complexity mass</th><th>High complexity mass share</th></tr>
<tr><td>all</td><td>{{.Metrics.All.CodeLines}}</td><td>{{.Metrics.All.Functions}}</td><td>{{.Metrics.All.Cyclomatic}}</td><td>{{.Metrics.All.MaxNesting}}</td><td>{{printf "%.6f" .Metrics.All.Mass}}</td><td>{{.Metrics.All.HighComplexityFunctions}}</td><td>{{printf "%.6f" .Metrics.All.HighComplexityMass}}</td><td>{{formatShare .Metrics.All.HighComplexityMassShare}}</td></tr>
<tr><td>production</td><td>{{.Metrics.Production.CodeLines}}</td><td>{{.Metrics.Production.Functions}}</td><td>{{.Metrics.Production.Cyclomatic}}</td><td>{{.Metrics.Production.MaxNesting}}</td><td>{{printf "%.6f" .Metrics.Production.Mass}}</td><td>{{.Metrics.Production.HighComplexityFunctions}}</td><td>{{printf "%.6f" .Metrics.Production.HighComplexityMass}}</td><td>{{formatShare .Metrics.Production.HighComplexityMassShare}}</td></tr>
<tr><td>test</td><td>{{.Metrics.Test.CodeLines}}</td><td>{{.Metrics.Test.Functions}}</td><td>{{.Metrics.Test.Cyclomatic}}</td><td>{{.Metrics.Test.MaxNesting}}</td><td>{{printf "%.6f" .Metrics.Test.Mass}}</td><td>{{.Metrics.Test.HighComplexityFunctions}}</td><td>{{printf "%.6f" .Metrics.Test.HighComplexityMass}}</td><td>{{formatShare .Metrics.Test.HighComplexityMassShare}}</td></tr>
</table>
<h2>Review inventory</h2>
<p>Function hotspots ({{.HotspotCount}} high-complexity of {{.FunctionCount}} functions) are ranked by raw mass with stable ties. Function inventory ({{.FunctionCount}}), native findings ({{.FindingCount}}), and Duplication clones ({{.CloneCount}}) are all included below.</p>
<p>Primary review items: {{.ReviewSummary.Total}}. Review status: unreviewed={{.ReviewSummary.Unreviewed}}, refactored={{.ReviewSummary.Refactored}}, retained={{.ReviewSummary.Retained}}, deferred={{.ReviewSummary.Deferred}}, stale={{.ReviewSummary.Stale}}. Non-hotspot function rows are raw observed inventory. A missing row never proves a comparison item was resolved.</p>
<div class="filters">
<label>Search<input id="inventory-search" type="search" placeholder="ID, path, reason, or upstream"></label>
<label>View<select id="inventory-view"><option value="review">review items</option><option value="">all inventory</option></select></label>
<label>Kind<select id="inventory-kind"><option value="">all</option><option value="function">function</option><option value="finding">finding</option><option value="clone">clone</option></select></label>
<label>Category<select id="inventory-category"><option value="">all</option>{{range .CategoryOptions}}<option value="{{.}}">{{.}}</option>{{end}}</select></label>
<label>Status<select id="inventory-status"><option value="">all</option>{{range .StatusOptions}}<option value="{{.}}">{{.}}</option>{{end}}</select></label>
</div>
<p>Showing <strong id="inventory-visible">{{.ReviewCount}}</strong> of {{len .InventoryRows}} inventory rows.</p>
<table id="review-inventory"><thead><tr><th>Kind</th><th>Category</th><th>Status</th><th>Location</th><th>ID / subject</th><th>Metrics</th><th>Reason</th><th>Evidence</th><th>Upstream</th><th>Previous</th></tr></thead><tbody>
{{range .InventoryRows}}<tr data-review-row data-review="{{.Review}}" data-kind="{{.Kind}}" data-category="{{.Category}}" data-status="{{.Status}}" data-stale="{{.Stale}}" data-search="{{.Search}}"><td>{{.Kind}}</td><td>{{.Category}}</td><td>{{.Status}}{{if .Stale}} (stale){{end}}</td><td><code>{{.Location}}</code></td><td><code>{{.ID}}</code>{{if .Fingerprint}}<br><code>fp: {{.Fingerprint}}</code>{{end}}<br>{{.Subject}}</td><td><code>{{.Metrics}}</code></td><td>{{if .ReasonCategory}}<code>{{.ReasonCategory}}</code>: {{end}}{{if .Reason}}{{.Reason}}{{else}}&mdash;{{end}}</td><td>{{if .Evidence}}{{.Evidence}}{{else}}&mdash;{{end}}</td><td>{{if .Upstream}}{{.Upstream}}{{else}}&mdash;{{end}}</td><td>{{if .Previous}}{{.Previous}}{{else}}&mdash;{{end}}</td></tr>{{else}}<tr><td colspan="10">No inventory rows.</td></tr>{{end}}
</tbody></table>
<script>
(function () {
  const rows = Array.from(document.querySelectorAll("[data-review-row]"));
  const search = document.getElementById("inventory-search");
  const view = document.getElementById("inventory-view");
  const kind = document.getElementById("inventory-kind");
  const category = document.getElementById("inventory-category");
  const status = document.getElementById("inventory-status");
  const visible = document.getElementById("inventory-visible");

  function update() {
    const query = search.value.trim().toLowerCase();
    const selectedView = view.value;
    const selectedKind = kind.value;
    const selectedCategory = category.value;
    const selectedStatus = status.value;
    let count = 0;
    rows.forEach((row) => {
      const matches = (!query || row.dataset.search.toLowerCase().includes(query)) &&
        (!selectedView || row.dataset.review === "true") &&
        (!selectedKind || row.dataset.kind === selectedKind) &&
        (!selectedCategory || row.dataset.category === selectedCategory) &&
        (!selectedStatus || row.dataset.status === selectedStatus);
      row.hidden = !matches;
      if (matches) {
        count++;
      }
    });
    visible.textContent = String(count);
  }

  [search, view, kind, category, status].forEach((control) => {
    control.addEventListener("input", update);
    control.addEventListener("change", update);
  });
  update();
})();
</script>
<h2>Duplication</h2>
{{if .Duplication}}<p>Status <code>{{.Duplication.Status}}</code>; tool <code>{{.Duplication.Config.Tool}}</code>; version <code>{{.Duplication.Config.Version}}</code>; {{.CloneCount}} clones are included in the complete review inventory above{{with .Duplication.Error}}; error <span class="error">{{.}}</span>{{end}}.</p>{{else}}<p>Status <code>not_requested</code>; no clone rows are available.</p>{{end}}
<details><summary>File inventory ({{len .Files}})</summary>
<table><tr><th>Path</th><th>Category</th><th>Status</th><th>Bytes</th><th>Lines</th><th>Code lines</th><th>Reason</th><th>Error</th></tr>
{{range .Files}}<tr><td><code>{{.Path}}</code></td><td>{{.Category}}</td><td>{{.Status}}</td><td>{{.Bytes}}</td><td>{{.Lines}}</td><td>{{.CodeLines}}</td><td>{{.Reason}}</td><td class="error">{{.Error}}</td></tr>{{else}}<tr><td colspan="8">No files.</td></tr>{{end}}
</table>
</details>
</body>
</html>
`
