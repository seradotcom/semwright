// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"io"
	"os"
	"path/filepath"
	"semwright-kicad-native/internal/wire"
	"strings"
	"syscall"
)

const DefaultConfig = "/workspace/kicad-config/connection.json"

type Instance struct {
	ID          string `json:"id"`
	Socket      string `json:"socket"`
	Token       string `json:"token,omitempty"`
	ExpectedPID int32  `json:"expected_pid,omitempty"`
}

func (Instance) String() string   { return "Instance{owner-configured; token redacted}" }
func (Instance) GoString() string { return "Instance{owner-configured; token redacted}" }

type AllowedDocument struct {
	BoardFilename string `json:"board_filename"`
	ProjectPath   string `json:"project_path"`
}
type Config struct {
	SchemaVersion    int               `json:"schema_version"`
	Instances        []Instance        `json:"instances"`
	SelectedInstance string            `json:"selected_instance"`
	AllowedDocuments []AllowedDocument `json:"allowed_documents"`
	EnableMutations  bool              `json:"enable_mutations"`
	TimeoutMS        int               `json:"timeout_ms"`
}

func (Config) String() string   { return "Config{private values redacted}" }
func (Config) GoString() string { return "Config{private values redacted}" }
func ParseConfig(data []byte) (Config, error) {
	var c Config
	if e := StrictJSON(data, &c); e != nil {
		return c, e
	}
	if c.SchemaVersion != 1 || len(c.Instances) > 16 || len(c.AllowedDocuments) > 32 || c.TimeoutMS < 50 || c.TimeoutMS > 2000 {
		return c, wire.E("InvalidArgument", "Configuration version or limits are invalid")
	}
	seen := map[string]bool{}
	paths := map[string]bool{}
	selected := c.SelectedInstance == ""
	for _, i := range c.Instances {
		if !slugPattern.MatchString(i.ID) || seen[i.ID] || paths[i.Socket] || len(i.Token) > 256 || strings.ContainsAny(i.Token, "\x00\r\n") || i.ExpectedPID < 0 {
			return c, wire.E("InvalidArgument", "Invalid or duplicate configured instance")
		}
		if !filepath.IsAbs(i.Socket) || filepath.Clean(i.Socket) != i.Socket || len(i.Socket) > 107 || strings.ContainsRune(i.Socket, '\x00') {
			return c, wire.E("InvalidArgument", "Invalid configured socket path")
		}
		seen[i.ID] = true
		paths[i.Socket] = true
		selected = selected || i.ID == c.SelectedInstance
	}
	if !selected {
		return c, wire.E("InvalidArgument", "Selected instance is not in the owner allowlist")
	}
	for _, d := range c.AllowedDocuments {
		if d.BoardFilename == "" || filepath.Base(d.BoardFilename) != d.BoardFilename || len(d.BoardFilename) > 256 || len(d.ProjectPath) > 4096 || !filepath.IsAbs(d.ProjectPath) || filepath.Clean(d.ProjectPath) != d.ProjectPath || strings.ContainsAny(d.BoardFilename+d.ProjectPath, "\x00\r\n") {
			return c, wire.E("InvalidArgument", "Invalid exact document allowlist")
		}
	}
	if c.EnableMutations && len(c.AllowedDocuments) == 0 {
		return c, wire.E("PermissionDenied", "Mutations require an exact owner-provided document allowlist")
	}
	return c, nil
}

// ReadConfig opens only the named mount file, never project files. O_NOFOLLOW and fstat close the final-component race.
func ReadConfig(path string) (Config, error) {
	empty := Config{SchemaVersion: 1, TimeoutMS: 750, Instances: []Instance{}, AllowedDocuments: []AllowedDocument{}}
	if path != DefaultConfig && !filepath.IsAbs(path) {
		return empty, wire.E("InvalidArgument", "Config path must be absolute")
	}
	if filepath.Clean(path) != path || strings.ContainsAny(path, "\x00\r\n") {
		return empty, wire.E("InvalidArgument", "Invalid canonical configuration path")
	}
	for ancestor := filepath.Dir(path); ancestor != "/"; ancestor = filepath.Dir(ancestor) {
		st, err := os.Lstat(ancestor)
		if os.IsNotExist(err) && path == DefaultConfig {
			return empty, nil
		}
		if err != nil || !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
			return empty, wire.E("PermissionDenied", "Invalid configuration ancestor")
		}
		uid := st.Sys().(*syscall.Stat_t).Uid
		if uid != 0 && uid != uint32(os.Getuid()) {
			return empty, wire.E("PermissionDenied", "Unexpected configuration ancestor owner")
		}
		if st.Mode().Perm()&0022 != 0 && !(uid == 0 && st.Mode()&os.ModeSticky != 0) {
			return empty, wire.E("PermissionDenied", "Configuration ancestor is writable by another principal")
		}
	}
	st, e := os.Lstat(path)
	if os.IsNotExist(e) && path == DefaultConfig {
		return empty, nil
	}
	if e != nil {
		return empty, wire.E("Unavailable", "Cannot read owner connection configuration")
	}
	if st.Mode()&os.ModeSymlink != 0 {
		return empty, wire.E("PermissionDenied", "Connection configuration cannot be a symlink")
	}
	fd, e := syscall.Open(path, syscall.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK|syscall.O_CLOEXEC, 0)
	if e != nil {
		return empty, wire.E("PermissionDenied", "Cannot securely open connection configuration")
	}
	f := os.NewFile(uintptr(fd), "connection-config")
	defer f.Close()
	info, e := f.Stat()
	if e != nil {
		return empty, wire.E("PermissionDenied", "Cannot inspect connection configuration")
	}
	s := info.Sys().(*syscall.Stat_t)
	if !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || s.Uid != uint32(os.Getuid()) || !os.SameFile(st, info) {
		return empty, wire.E("PermissionDenied", "Configuration must be an unchanged owner-only regular file")
	}
	b, e := io.ReadAll(io.LimitReader(f, 65537))
	if e != nil || len(b) > 65536 {
		return empty, wire.E("ResourceExhausted", "Connection configuration exceeds size budget")
	}
	return ParseConfig(b)
}
