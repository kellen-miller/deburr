package analysis

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"sort"
	"strconv"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

const cloneEndpointCount = 2

const cloneFileScope = "file"

const cloneAmbiguousMinimum = 2

type cloneSourceScope struct {
	source    []byte
	fileSet   *token.FileSet
	astFile   *ast.File
	functions []*functionNode
}

type normalizedCloneEndpoint struct {
	scope         string
	tokens        string
	baseKey       string
	identity      string
	contentDigest string
	location      report.Location
	occurrence    int
	ambiguous     bool
}

type normalizedClonePair struct {
	category    report.Category
	pairKey     string
	familyID    string
	fingerprint string
	first       normalizedCloneEndpoint
	second      normalizedCloneEndpoint
	pairOrdinal int
	tokens      int
	lines       int
	ambiguous   bool
}

func buildCloneSourceScopes(selected map[string][]byte) map[string]cloneSourceScope {
	scopes := make(map[string]cloneSourceScope, len(selected))
	for path, source := range selected {
		fileSet := token.NewFileSet()
		parsed, err := parser.ParseFile(fileSet, path, source, parser.ParseComments)
		if err != nil || parsed == nil {
			scopes[path] = cloneSourceScope{source: source, fileSet: fileSet}
			continue
		}

		file := &sourceFile{
			path:    path,
			source:  source,
			fileSet: fileSet,
			astFile: parsed,
		}
		nodes := collectFunctions(file)
		assignFunctionNames(nodes)
		scopes[path] = cloneSourceScope{
			source:    source,
			fileSet:   fileSet,
			astFile:   parsed,
			functions: nodes,
		}
	}

	return scopes
}

func normalizeCloneEndpoint(location report.Location, scope cloneSourceScope) (normalizedCloneEndpoint, error) {
	start, end, ok := sourceRegionOffsets(scope.source, location)
	if !ok {
		return normalizedCloneEndpoint{}, fmt.Errorf(
			"duplication report returned invalid source region %q",
			locationKey(location),
		)
	}

	tokens := normalizedSourceTokenRegion(scope.source, location)
	if tokens == "" || end <= start {
		return normalizedCloneEndpoint{}, fmt.Errorf(
			"duplication report returned empty source region %q",
			locationKey(location),
		)
	}

	functionScope, scopeAmbiguous := cloneFunctionScope(scope, location)
	baseKey := location.Path + "\x00" + functionScope + "\x00" + tokens
	contentDigest := digestText("clone-endpoint\x00" + location.Path + "\x00" + functionScope + "\x00" + tokens)
	return normalizedCloneEndpoint{
		location:      location,
		scope:         functionScope,
		tokens:        tokens,
		baseKey:       baseKey,
		contentDigest: contentDigest,
		ambiguous:     scopeAmbiguous,
	}, nil
}

func normalizedSourceTokenRegion(source []byte, location report.Location) string {
	start, end, ok := sourceRegionOffsets(source, location)
	if !ok {
		return ""
	}

	return normalizedSourceTokens(source[start:end])
}

func sourceRegionOffsets(source []byte, location report.Location) (int, int, bool) {
	start, ok := sourcePositionOffset(source, location.Start, false)
	if !ok {
		return 0, 0, false
	}

	end, ok := sourcePositionOffset(source, location.End, true)
	if !ok || end < start {
		return 0, 0, false
	}

	if start > len(source) {
		start = len(source)
	}
	if end > len(source) {
		end = len(source)
	}
	return start, end, true
}

func sourcePositionOffset(source []byte, position report.Position, inclusiveEnd bool) (int, bool) {
	if position.Line <= 0 || position.Column <= 0 {
		return 0, false
	}

	lineStart := 0
	for line := 1; line <= position.Line; line++ {
		lineEnd := lineStart
		for lineEnd < len(source) && source[lineEnd] != '\n' {
			lineEnd++
		}
		if line == position.Line {
			offset := lineStart + position.Column - 1
			if inclusiveEnd {
				offset++
			}
			return offset, offset >= lineStart && offset <= lineEnd
		}
		if lineEnd == len(source) {
			return 0, false
		}
		lineStart = lineEnd + 1
	}

	return 0, false
}

func cloneFunctionScope(scope cloneSourceScope, location report.Location) (string, bool) {
	if scope.astFile == nil || len(scope.functions) == 0 {
		if scope.astFile != nil && scope.astFile.Name != nil {
			return scope.astFile.Name.Name + ".file", false
		}
		return cloneFileScope, false
	}

	start, end, ok := sourceRegionOffsets(scope.source, location)
	if !ok || scope.fileSet == nil {
		return cloneFileScope, false
	}

	file := scope.fileSet.File(scope.astFile.Pos())
	if file == nil {
		return cloneFileScope, false
	}
	startPos := int(file.Pos(start))
	endPos := int(file.Pos(end))
	var owner *functionNode
	for _, function := range scope.functions {
		if function.start > startPos || function.end < endPos {
			continue
		}
		if owner == nil || function.end-function.start < owner.end-owner.start {
			owner = function
		}
	}
	if owner != nil {
		return functionIdentityName(owner), owner.ambiguous
	}

	if scope.astFile.Name != nil {
		return scope.astFile.Name.Name + ".file", false
	}
	return cloneFileScope, false
}

func endpointLess(left, right *normalizedCloneEndpoint) bool {
	if left.baseKey != right.baseKey {
		return left.baseKey < right.baseKey
	}

	return locationKey(left.location) < locationKey(right.location)
}

func assignCloneEndpointOccurrences(pairs []normalizedClonePair) {
	endpoints := make([]*normalizedCloneEndpoint, 0, len(pairs)*cloneEndpointCount)
	seen := make(map[string]*normalizedCloneEndpoint, len(pairs)*cloneEndpointCount)
	for index := range pairs {
		for _, endpoint := range []*normalizedCloneEndpoint{&pairs[index].first, &pairs[index].second} {
			key := endpoint.baseKey + "\x00" + locationKey(endpoint.location)
			if _, ok := seen[key]; ok {
				continue
			}
			seen[key] = endpoint
			endpoints = append(endpoints, endpoint)
		}
	}

	sort.Slice(endpoints, func(left, right int) bool {
		if endpoints[left].baseKey != endpoints[right].baseKey {
			return endpoints[left].baseKey < endpoints[right].baseKey
		}
		return locationKey(endpoints[left].location) < locationKey(endpoints[right].location)
	})

	counts := make(map[string]int)
	for _, endpoint := range endpoints {
		baseKey := endpoint.baseKey
		endpoint.occurrence = counts[baseKey]
		counts[baseKey]++
		endpoint.ambiguous = endpoint.ambiguous || counts[baseKey] >= cloneAmbiguousMinimum
		endpoint.identity = fmt.Sprintf("%s#%d", endpoint.contentDigest, endpoint.occurrence)
	}
	for _, endpoint := range endpoints {
		endpoint.ambiguous = endpoint.ambiguous || counts[endpoint.baseKey] >= cloneAmbiguousMinimum
	}
	for index := range pairs {
		for _, endpoint := range []*normalizedCloneEndpoint{&pairs[index].first, &pairs[index].second} {
			key := endpoint.baseKey + "\x00" + locationKey(endpoint.location)
			if canonical, ok := seen[key]; ok {
				*endpoint = *canonical
			}
		}
	}
}

func assignClonePairIDs(pairs []normalizedClonePair) {
	sort.SliceStable(pairs, func(left, right int) bool {
		return clonePairSortKey(&pairs[left]) < clonePairSortKey(&pairs[right])
	})
	assignCloneFamilies(pairs)

	counts := make(map[string]int, len(pairs))
	for index := range pairs {
		pair := &pairs[index]
		pair.pairKey = clonePairSortKey(pair)
		pair.pairOrdinal = counts[pair.pairKey]
		counts[pair.pairKey]++
		pair.fingerprint = clonePairFingerprint(pair)
	}
	for index := range pairs {
		pairs[index].ambiguous = counts[pairs[index].pairKey] >= cloneAmbiguousMinimum
	}
}

func assignCloneFamilies(pairs []normalizedClonePair) {
	parents := make([]int, len(pairs))
	for index := range parents {
		parents[index] = index
	}
	endpointOwners := make(map[string]int, len(pairs)*cloneEndpointCount)
	for index := range pairs {
		for _, endpoint := range []string{pairs[index].first.identity, pairs[index].second.identity} {
			owner, ok := endpointOwners[endpoint]
			if !ok {
				endpointOwners[endpoint] = index
				continue
			}

			cloneUnion(parents, index, owner)
		}
	}

	components := make(map[int][]int, len(pairs))
	for index := range pairs {
		root := cloneFind(parents, index)
		components[root] = append(components[root], index)
	}
	for _, component := range components {
		sort.Slice(component, func(left, right int) bool {
			return clonePairSortKey(&pairs[component[left]]) < clonePairSortKey(&pairs[component[right]])
		})
		memberKeys := make([]string, 0, len(component))
		for _, index := range component {
			memberKeys = append(memberKeys, clonePairSortKey(&pairs[index]))
		}
		familyInput := "clone-family-v2\x00" +
			string(pairs[component[0]].category) + "\x00" + strings.Join(memberKeys, "\x00")
		familyID := "family-" + digestText("clone-family-id\x00" + familyInput)[:16]
		for _, index := range component {
			pairs[index].familyID = familyID
		}
	}
}

func cloneFind(parents []int, index int) int {
	root := index
	for parents[root] != root {
		root = parents[root]
	}

	for parents[index] != index {
		next := parents[index]
		parents[index] = root
		index = next
	}
	return root
}

func cloneUnion(parents []int, left, right int) {
	leftRoot := cloneFind(parents, left)
	rightRoot := cloneFind(parents, right)
	if leftRoot != rightRoot {
		parents[rightRoot] = leftRoot
	}
}

func clonePairSortKey(pair *normalizedClonePair) string {
	return pair.first.identity + "\x00" + pair.second.identity
}

func clonePairFingerprint(pair *normalizedClonePair) string {
	endpoints := []string{pair.first.contentDigest, pair.second.contentDigest}
	sort.Strings(endpoints)
	return digestText("clone-pair-v2\x00" + string(pair.category) + "\x00" + strings.Join(endpoints, "\x00"))
}

func clonePairID(pair *normalizedClonePair) string {
	return "clone-" + digestText(
		string(pair.category) + "\x00" + pair.first.identity + "\x00" + pair.second.identity + "\x00" +
			strconv.Itoa(pair.pairOrdinal),
	)[:16]
}

func digestText(value string) string {
	digest := sha256.Sum256([]byte(value))
	return hex.EncodeToString(digest[:])
}

func locationKey(location report.Location) string {
	return fmt.Sprintf(
		"%s:%d:%d-%d:%d",
		location.Path,
		location.Start.Line,
		location.Start.Column,
		location.End.Line,
		location.End.Column,
	)
}
