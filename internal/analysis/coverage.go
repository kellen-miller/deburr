package analysis

import (
	"crypto/sha256"
	"fmt"
	"sort"
	"strconv"

	"github.com/kellen-miller/deburr/internal/report"
)

func sourceManifestDigest(files []*sourceFile) string {
	ordered := append([]*sourceFile(nil), files...)
	sort.SliceStable(ordered, func(left, right int) bool {
		return ordered[left].path < ordered[right].path
	})

	hash := sha256.New()
	for _, file := range ordered {
		path := slashPath(file.path)
		frame := strconv.Itoa(len(path)) + ":" + path + ":" + strconv.Itoa(len(file.source)) + ":"
		_, _ = hash.Write([]byte(frame))
		_, _ = hash.Write(file.source)
		_, _ = hash.Write([]byte{'\x00'})
	}

	return fmt.Sprintf("sha256:%x", hash.Sum(nil))
}

func buildFileReports(files []*sourceFile) ([]report.File, []report.CategoryCoverage) {
	reports := make([]report.File, 0, len(files))
	categoryCounts := make(map[report.Category]*report.CategoryCoverage, len(coverageCategories()))
	for _, category := range coverageCategories() {
		categoryCounts[category] = &report.CategoryCoverage{Category: category}
	}

	for index := range files {
		file := files[index]
		reports = append(reports, report.File{
			Path:      file.path,
			Category:  file.category,
			Status:    file.status,
			Bytes:     file.bytes,
			Lines:     file.lines,
			CodeLines: file.codeLines,
			Reason:    file.reason,
			Error:     file.err,
		})

		category := file.category
		if category == "" {
			category = report.CategoryUnsupported
		}

		counts := categoryCounts[category]
		if counts == nil {
			counts = &report.CategoryCoverage{Category: category}
			categoryCounts[category] = counts
		}

		countFileCategory(counts, file.status)
	}

	byCategory := make([]report.CategoryCoverage, 0, len(categoryCounts))
	for _, category := range coverageCategories() {
		byCategory = append(byCategory, *categoryCounts[category])
	}

	return reports, byCategory
}

func coverageCategories() []report.Category {
	return []report.Category{
		report.CategoryProduction,
		report.CategoryTest,
		report.CategoryTestdata,
		report.CategoryGenerated,
		report.CategoryVendor,
		report.CategoryUnsupported,
	}
}

func countFileCategory(counts *report.CategoryCoverage, status report.FileStatus) {
	counts.Discovered++
	switch status {
	case report.FileAnalyzed:
		counts.Analyzed++
	case report.FileExcluded:
		counts.Excluded++
	case report.FileUnsupported:
		counts.Unsupported++
	case report.FileReadError:
		counts.ReadErrors++
	case report.FileParseError:
		counts.ParseErrors++
	}
}

func calculateMetrics(files []report.File, functions []report.Function) report.Metrics {
	metrics := report.Metrics{}
	for _, file := range files {
		switch file.Status {
		case report.FileAnalyzed:
			metrics.All.CodeLines += file.CodeLines
		default:
			continue
		}

		switch file.Category {
		case report.CategoryProduction:
			metrics.Production.CodeLines += file.CodeLines
		case report.CategoryTest:
			metrics.Test.CodeLines += file.CodeLines
		default:
			continue
		}
	}

	for index := range functions {
		function := &functions[index]
		addFunctionMetric(&metrics.All, function)
		switch function.Category {
		case report.CategoryProduction:
			addFunctionMetric(&metrics.Production, function)
		case report.CategoryTest:
			addFunctionMetric(&metrics.Test, function)
		case report.CategoryTestdata, report.CategoryGenerated, report.CategoryVendor, report.CategoryUnsupported:
		}
	}

	setMassShare(&metrics.All)
	setMassShare(&metrics.Production)
	setMassShare(&metrics.Test)
	return metrics
}

func addFunctionMetric(bucket *report.MetricBucket, function *report.Function) {
	bucket.Functions++
	bucket.Cyclomatic += function.Cyclomatic
	bucket.MaxNesting = max(bucket.MaxNesting, function.MaxNesting)
	bucket.Mass += function.Mass
	if function.HighComplexity {
		bucket.HighComplexityFunctions++
		bucket.HighComplexityMass += function.Mass
	}
}

func setMassShare(bucket *report.MetricBucket) {
	if bucket.Mass == 0 {
		bucket.HighComplexityMassShare = nil
		return
	}

	share := bucket.HighComplexityMass / bucket.Mass
	bucket.HighComplexityMassShare = &share
}
