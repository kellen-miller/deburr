package analysis

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

func (s *analysisState) relative(path string) string {
	rel, err := filepath.Rel(s.base, path)
	if err != nil {
		return filepath.Base(path)
	}

	return slashPath(rel)
}

func categoryForPath(path string) report.Category {
	parts := strings.Split(slashPath(path), "/")
	for _, part := range parts[:len(parts)-1] {
		if part == "vendor" {
			return report.CategoryVendor
		}

		if part == "testdata" {
			return report.CategoryTestdata
		}
	}

	if strings.HasSuffix(parts[len(parts)-1], "_test.go") {
		return report.CategoryTest
	}

	return report.CategoryProduction
}

func skipDirectory(path string) bool {
	parts := strings.Split(slashPath(path), "/")
	name := parts[len(parts)-1]
	switch name {
	case ".git", ".hg", ".svn", "node_modules", ".venv", "venv", ".agent", ".worktrees":
		return true
	default:
		return false
	}
}

func slashPath(path string) string {
	path = filepath.ToSlash(path)
	if path == "." {
		return path
	}

	return strings.TrimPrefix(path, "./")
}

func cleanError(err error) string {
	if err == nil {
		return ""
	}

	if pathErr, ok := errors.AsType[*os.PathError](err); ok {
		return pathErr.Err.Error()
	}

	return err.Error()
}
