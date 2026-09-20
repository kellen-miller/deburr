package analysis

import (
	"bytes"
	"go/ast"
	"go/parser"
	"go/scanner"
	"go/token"
)

func sourceLineCounts(filename string, source []byte) (int, int) {
	if len(source) == 0 {
		return 0, 0
	}

	lines := physicalLineCount(source)
	lineCode := sourceLineMap(filename, source)
	codeLines := 0
	for _, code := range lineCode {
		if code {
			codeLines++
		}
	}

	return lines, codeLines
}

func physicalLineCount(source []byte) int {
	lines := bytes.Count(source, []byte{'\n'})
	if len(source) > 0 && source[len(source)-1] != '\n' {
		lines++
	}

	return lines
}

func sourceLineMap(filename string, source []byte) []bool {
	lineCode := make([]bool, physicalLineCount(source)+1)
	file := token.NewFileSet().AddFile(filename, -1, len(source))
	var sourceScanner scanner.Scanner
	sourceScanner.Init(file, source, nil, scanner.ScanComments)
	for {
		pos, tok, lit := sourceScanner.Scan()
		if tok == token.EOF {
			return lineCode
		}

		if !tokenMarksCode(tok, lit) {
			continue
		}

		line := file.PositionFor(pos, false).Line
		endLine := tokenEndLine(file, source, pos, tok, lit)
		for codeLine := line; codeLine <= endLine && codeLine > 0 && codeLine < len(lineCode); codeLine++ {
			lineCode[codeLine] = true
		}
	}
}

func tokenMarksCode(tok token.Token, lit string) bool {
	if tok == token.COMMENT {
		return false
	}

	return tok != token.SEMICOLON || lit != ""
}

func tokenEndLine(file *token.File, source []byte, pos token.Pos, tok token.Token, lit string) int {
	line := file.PositionFor(pos, false).Line
	if lit == "" {
		return line
	}

	startOffset := file.Offset(pos)
	endOffset := startOffset + len(lit) - 1
	if tok == token.STRING && lit[0] == '`' {
		if closing := bytes.IndexByte(source[startOffset+1:], '`'); closing >= 0 {
			endOffset = startOffset + 1 + closing
		}
	}

	if endOffset >= len(source) {
		endOffset = len(source) - 1
	}

	return file.PositionFor(file.Pos(endOffset), false).Line
}

func isGenerated(source []byte) bool {
	file, err := parser.ParseFile(
		token.NewFileSet(),
		"generated.go",
		source,
		parser.ParseComments|parser.PackageClauseOnly,
	)
	return err == nil && file != nil && ast.IsGenerated(file)
}
