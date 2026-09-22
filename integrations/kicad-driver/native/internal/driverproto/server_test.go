// SPDX-License-Identifier: GPL-3.0-or-later
package driverproto

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"semwright-kicad-native/internal/core"
	"testing"
)

func TestFrameSizeAndTruncation(t *testing.T) {
	for _, n := range []uint32{0, 1048577} {
		var h [4]byte
		binary.BigEndian.PutUint32(h[:], n)
		if _, e := Read(bytes.NewReader(h[:])); e == nil {
			t.Fatal(n)
		}
	}
	if _, e := Read(bytes.NewReader([]byte{0, 0, 0, 3, 1})); e == nil {
		t.Fatal("truncated")
	}
}
func TestTypedRequestUnknownAndDuplicateFields(t *testing.T) {
	for _, s := range []string{`{"type":"health","id":"1","extra":1}`, `{"type":"health","id":"1","id":"2"}`, `{"type":"health"}`, `{"type":"raw","id":"1"}`, `{"type":"health","id":""}`} {
		if _, e := decode([]byte(s)); e == nil {
			t.Fatal(s)
		}
	}
}
func TestFullOfflineHandshakeShutdown(t *testing.T) {
	in := bytes.NewBuffer(nil)
	out := bytes.NewBuffer(nil)
	hello := map[string]any{"type": "hello", "protocol": 1, "provider": map[string]any{"id": core.ProviderID, "kind": "driver", "version": core.DriverVersion, "namespace": core.Namespace, "application": nil, "origin": "driver-manifest:test"}, "executable_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
	_ = Write(in, hello)
	_ = Write(in, map[string]any{"type": "capabilities", "id": "1"})
	_ = Write(in, map[string]any{"type": "health", "id": "2"})
	_ = Write(in, map[string]any{"type": "shutdown", "id": "3"})
	if e := Serve(in, out, core.Config{SchemaVersion: 1, TimeoutMS: 100}); e != nil {
		t.Fatal(e)
	}
	for _, kind := range []string{"ready", "capabilities", "healthy", "shutdown"} {
		b, e := Read(out)
		if e != nil {
			t.Fatal(e)
		}
		var v map[string]any
		_ = json.Unmarshal(b, &v)
		if v["type"] != kind {
			t.Fatal(v)
		}
	}
}
func TestHelloRequired(t *testing.T) {
	in := bytes.NewBuffer(nil)
	_ = Write(in, map[string]any{"type": "health", "id": "1"})
	if e := Serve(in, bytes.NewBuffer(nil), core.Config{}); e == nil {
		t.Fatal("missing handshake")
	}
}
func FuzzDriverFrame(f *testing.F) {
	f.Add([]byte(`{"type":"health","id":"1"}`))
	f.Add([]byte{})
	f.Fuzz(func(t *testing.T, b []byte) {
		DecodeProbe(b)
		if len(b) < 1048600 {
			_, _ = Read(bytes.NewReader(b))
		}
	})
}
