// SPDX-License-Identifier: GPL-3.0-or-later
// Wire compatibility harness. Production Rust uses the upstream SDK, not this package.
package driverproto

import (
	"encoding/binary"
	"encoding/json"
	"io"
	"semwright-kicad-native/internal/core"
	"semwright-kicad-native/internal/wire"
	"strings"
	"unicode"
)

type Provider struct {
	ID          string  `json:"id"`
	Kind        string  `json:"kind"`
	Version     string  `json:"version"`
	Namespace   string  `json:"namespace"`
	Application *string `json:"application"`
	Origin      string  `json:"origin"`
}
type request struct {
	Type       string         `json:"type"`
	Protocol   uint32         `json:"protocol"`
	Provider   Provider       `json:"provider"`
	Executable string         `json:"executable_sha256"`
	ID         string         `json:"id"`
	Command    string         `json:"command"`
	Digest     string         `json:"descriptor_sha256"`
	Args       map[string]any `json:"args"`
}

func validText(v string, max int) bool {
	return v != "" && len(v) <= max && !strings.ContainsFunc(v, unicode.IsControl)
}
func decode(b []byte) (request, error) {
	var raw map[string]json.RawMessage
	var out request
	if err := core.StrictJSON(b, &raw); err != nil {
		return out, err
	}
	if err := core.StrictJSON(b, &out); err != nil {
		return out, err
	}
	allowed := map[string][]string{"hello": {"type", "protocol", "provider", "executable_sha256"}, "capabilities": {"type", "id"}, "health": {"type", "id"}, "execute": {"type", "id", "command", "descriptor_sha256", "args"}, "shutdown": {"type", "id"}}
	fields, ok := allowed[out.Type]
	if !ok {
		return out, wire.E("ProtocolMismatch", "Unknown driver request type")
	}
	if len(raw) != len(fields) {
		return out, wire.E("InvalidArgument", "Missing or unexpected request field")
	}
	for _, f := range fields {
		if _, ok := raw[f]; !ok {
			return out, wire.E("InvalidArgument", "Missing typed request field")
		}
	}
	if out.Type != "hello" && !validText(out.ID, 128) {
		return out, wire.E("InvalidArgument", "Invalid request identifier")
	}
	return out, nil
}
func Read(r io.Reader) ([]byte, error) {
	var h [4]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(h[:])
	if n == 0 || n > wire.MaxMessage {
		return nil, wire.E("ResourceExhausted", "Invalid driver frame size")
	}
	b := make([]byte, n)
	_, err := io.ReadFull(r, b)
	return b, err
}
func Write(w io.Writer, v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	if len(b) == 0 || len(b) > wire.MaxMessage {
		return wire.E("ResourceExhausted", "Driver response exceeds frame budget")
	}
	var h [4]byte
	binary.BigEndian.PutUint32(h[:], uint32(len(b)))
	if _, err = w.Write(h[:]); err != nil {
		return err
	}
	_, err = w.Write(b)
	return err
}
func Serve(r io.Reader, w io.Writer, c core.Config) error {
	b, err := Read(r)
	if err != nil {
		return err
	}
	hello, err := decode(b)
	if err != nil {
		return err
	}
	p := hello.Provider
	if hello.Type != "hello" || hello.Protocol != 1 || p.ID != core.ProviderID || p.Kind != "driver" || p.Namespace != core.Namespace || p.Version != core.DriverVersion || !validText(p.Origin, 256) || (p.Application != nil && !validText(*p.Application, 256)) || len(hello.Executable) != 64 {
		return wire.E("ProtocolMismatch", "Driver handshake identity mismatch")
	}
	e, err := core.New(c)
	if err != nil {
		return err
	}
	defer e.Close()
	if err = Write(w, map[string]any{"type": "ready", "protocol": 1, "id": "kicad", "version": core.DriverVersion}); err != nil {
		return err
	}
	for {
		b, err = Read(r)
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		req, err := decode(b)
		if err != nil {
			return err
		}
		var value any
		switch req.Type {
		case "capabilities":
			caps, x := e.Capabilities()
			err = x
			value = map[string]any{"type": "capabilities", "id": req.ID, "capabilities": caps, "digest": core.Digest(caps)}
		case "execute":
			v, x := e.Execute(req.Command, req.Digest, req.Args)
			err = x
			value = map[string]any{"type": "result", "id": req.ID, "value": v}
		case "health":
			v, x := e.Health()
			err = x
			value = map[string]any{"type": "healthy", "id": req.ID, "details": v}
		case "shutdown":
			return Write(w, map[string]any{"type": "shutdown", "id": req.ID})
		default:
			return wire.E("ProtocolMismatch", "Repeated driver handshake")
		}
		if err != nil {
			value = map[string]any{"type": "failure", "id": req.ID, "error": wire.AsError(err)}
		}
		if err = Write(w, value); err != nil {
			return err
		}
	}
}

// DecodeProbe is a parser-only entry point for bounded fuzz tests, not a driver capability.
func DecodeProbe(b []byte) { _, _ = decode(b) }
