package analysis

import (
	"fmt"
	"path/filepath"
	"sort"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

func normalizeCPDClones(
	duplicates []cpdDuplicate,
	category report.Category,
	snapshotRoot string,
	selected map[string][]byte,
	scopes map[string]*cloneSourceScope,
) ([]report.Clone, error) {
	pairs := make([]normalizedClonePair, 0, len(duplicates))
	for index := range duplicates {
		duplicate := duplicates[index]
		first, err := normalizeCPDLocation(duplicate.FirstFile, category, snapshotRoot, selected)
		if err != nil {
			return nil, err
		}

		second, err := normalizeCPDLocation(duplicate.SecondFile, category, snapshotRoot, selected)
		if err != nil {
			return nil, err
		}

		firstEndpoint, err := normalizeCloneEndpoint(first, scopes[first.Path])
		if err != nil {
			return nil, err
		}
		secondEndpoint, err := normalizeCloneEndpoint(second, scopes[second.Path])
		if err != nil {
			return nil, err
		}
		if endpointLess(&secondEndpoint, &firstEndpoint) {
			firstEndpoint, secondEndpoint = secondEndpoint, firstEndpoint
		}

		pairs = append(pairs, normalizedClonePair{
			first:    firstEndpoint,
			second:   secondEndpoint,
			category: category,
			tokens:   duplicate.Tokens,
			lines:    duplicate.Lines,
		})
	}

	assignCloneEndpointOccurrences(pairs)
	assignClonePairIDs(pairs)

	clones := make([]report.Clone, 0, len(pairs))
	for index := range pairs {
		pair := &pairs[index]
		locations := []report.Location{pair.first.location, pair.second.location}
		clones = append(clones, report.Clone{
			ID:                clonePairID(pair),
			FamilyID:          pair.familyID,
			Fingerprint:       pair.fingerprint,
			Category:          category,
			Tokens:            pair.tokens,
			Lines:             pair.lines,
			Locations:         locations,
			IdentityAmbiguous: pair.ambiguous || pair.first.ambiguous || pair.second.ambiguous,
		})
	}

	sort.Slice(clones, func(i, j int) bool { return clones[i].ID < clones[j].ID })
	return clones, nil
}

func normalizeCPDLocation(
	file cpdFile,
	category report.Category,
	snapshotRoot string,
	selected map[string][]byte,
) (report.Location, error) {
	path := filepath.ToSlash(file.Name)
	prefix := filepath.ToSlash(snapshotRoot) + "/"
	path = strings.TrimPrefix(path, prefix)
	path = strings.TrimPrefix(path, string(category)+"/")
	source, ok := selected[path]
	if !ok {
		matches := make([]string, 0, 1)
		for candidate := range selected {
			if path == candidate || strings.HasSuffix(path, "/"+candidate) {
				matches = append(matches, candidate)
			}
		}

		if len(matches) == 1 {
			path = matches[0]
			source = selected[path]
		} else {
			return report.Location{}, fmt.Errorf(
				"duplication report returned unknown source path %q",
				filepath.Base(path),
			)
		}
	}

	start := report.Position{Line: file.StartLoc.Line, Column: file.StartLoc.Column + 1}
	end := cpdEndPosition(file.EndLoc, source)
	if start.Line == 0 {
		start.Line = file.Start
	}

	if start.Column == 0 {
		start.Column = 1
	}

	if end.Line == 0 {
		end.Line = file.End
	}

	if end.Column == 0 {
		end.Column = 1
	}

	return report.Location{Path: path, Start: start, End: end}, nil
}

func cpdEndPosition(point cpdPoint, source []byte) report.Position {
	if point.Line == 0 {
		return report.Position{}
	}

	if point.Column > 0 {
		return report.Position{Line: point.Line, Column: point.Column}
	}

	if point.Line == 1 {
		return report.Position{Line: 1, Column: 1}
	}

	line := point.Line - 1
	return report.Position{Line: line, Column: sourceLineLength(source, line)}
}

func sourceLineLength(source []byte, line int) int {
	if line <= 0 {
		return 0
	}

	current := 1
	start := 0
	for offset, value := range source {
		if value != '\n' {
			continue
		}

		if current == line {
			end := offset
			if end > start && source[end-1] == '\r' {
				end--
			}

			return end - start
		}

		current++
		start = offset + 1
	}

	if current == line {
		end := len(source)
		if end > start && source[end-1] == '\r' {
			end--
		}

		return end - start
	}

	return 0
}
