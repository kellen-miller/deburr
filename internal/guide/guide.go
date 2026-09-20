package guide

import _ "embed"

// cleanup is the canonical cleanup workflow exposed by `deburr guide cleanup`.
//
//go:embed cleanup.md
var cleanup string

// Cleanup returns the portable cleanup workflow.
func Cleanup() string {
	return cleanup
}
