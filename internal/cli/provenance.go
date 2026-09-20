package cli

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime/debug"
	"strings"

	"github.com/kellen-miller/deburr/internal/report"
)

func applyProvenance(value *report.Report, target string) {
	manifest := value.Provenance.Source.ManifestDigest
	value.Provenance.Build = currentBuildIdentity()
	value.Provenance.Source = currentSourceIdentity(target)
	value.Provenance.Source.ManifestDigest = manifest
}

func currentBuildIdentity() report.BuildIdentity {
	identity := report.BuildIdentity{}
	info, ok := debug.ReadBuildInfo()
	if !ok || info == nil {
		return identity
	}

	hasRevision := false
	hasModified := false
	for _, setting := range info.Settings {
		switch setting.Key {
		case "vcs.revision":
			identity.Revision = setting.Value
			hasRevision = identity.Revision != ""
		case "vcs.modified":
			switch setting.Value {
			case "true":
				identity.Modified = true
				hasModified = true
			case "false":
				identity.Modified = false
				hasModified = true
			}
		}
	}

	identity.Known = hasRevision && hasModified
	return identity
}

func currentSourceIdentity(target string) report.SourceIdentity {
	identity := report.SourceIdentity{}
	directory := target
	if info, err := os.Stat(directory); err == nil && !info.IsDir() {
		directory = filepath.Dir(directory)
	}

	commit, commitErr := gitOutput(directory, "rev-parse", "HEAD")
	status, statusErr := gitOutput(directory, "status", "--porcelain=v1", "--untracked-files=all")
	if commitErr != nil || statusErr != nil {
		return identity
	}

	identity.Commit = strings.TrimSpace(commit)
	identity.Dirty = strings.TrimSpace(status) != ""
	identity.GitKnown = identity.Commit != ""
	if !identity.GitKnown {
		identity.Dirty = false
	}

	return identity
}

func gitOutput(directory string, args ...string) (string, error) {
	commandPrefix := []string{"-c", "core.fsmonitor=false", "-C", directory}
	commandArgs := make([]string, 0, len(commandPrefix)+len(args))
	commandArgs = append(commandArgs, commandPrefix...)
	commandArgs = append(commandArgs, args...)
	command := exec.CommandContext(context.Background(), "git", commandArgs...)
	command.Env = append(os.Environ(), "GIT_OPTIONAL_LOCKS=0")
	output, err := command.Output()
	if err != nil {
		return "", fmt.Errorf("git command failed: %w", err)
	}

	return string(output), nil
}
