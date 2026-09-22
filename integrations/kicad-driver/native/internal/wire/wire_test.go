// SPDX-License-Identifier: GPL-3.0-or-later
package wire

import (
	"bytes"
	"encoding/binary"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestVarintAndSignedBoundaries(t *testing.T) {
	for _, v := range []uint64{0, 1, 127, 128, 255, 65535, 1 << 63, ^uint64(0)} {
		m, e := Parse(V(1, v))
		if e != nil {
			t.Fatal(e)
		}
		n, e := m.Number(1)
		if e != nil || n != v {
			t.Fatalf("%d %d %v", v, n, e)
		}
	}
}
func TestUnknownFieldsPreserved(t *testing.T) {
	unknown := Join(B(123, []byte("future")), V(444, 99))
	original := Join(V(1, 1), unknown)
	changed, e := Replace(original, 1, V(1, 3))
	if e != nil || !bytes.Equal(changed, Join(V(1, 3), unknown)) {
		t.Fatal(e)
	}
}
func TestRejectMalformedWire(t *testing.T) {
	for _, b := range [][]byte{{0}, {8, 128}, {15}, {10, 10, 1}, {9, 1}, {13, 1}, bytes.Repeat([]byte{255}, 12)} {
		if _, e := Parse(b); e == nil {
			t.Fatalf("accepted malformed %x", b)
		}
	}
}
func TestRejectDuplicateSingular(t *testing.T) {
	m, _ := Parse(Join(V(1, 1), V(1, 2)))
	if _, e := m.Number(1); e == nil {
		t.Fatal("duplicate accepted")
	}
	if _, e := Replace(Join(V(1, 1), V(1, 2)), 1, V(1, 3)); e == nil {
		t.Fatal("duplicate replacement")
	}
}
func TestRejectOversizedWire(t *testing.T) {
	if _, e := Parse(make([]byte, MaxMessage+1)); AsError(e).Code != "ResourceExhausted" {
		t.Fatal(e)
	}
}
func TestFieldBudget(t *testing.T) {
	if _, e := Parse(bytes.Repeat([]byte{8, 1}, MaxFields+1)); AsError(e).Code != "ResourceExhausted" {
		t.Fatal(e)
	}
}
func TestPackedAndUTF8(t *testing.T) {
	m, _ := Parse(Join(B(2, Join(Uint(1), Uint(128))), V(2, 9)))
	out, e := m.Packed(2, 3)
	if e != nil || len(out) != 3 {
		t.Fatal(e)
	}
	if _, e = m.Packed(2, 2); e == nil {
		t.Fatal("budget")
	}
	m, _ = Parse(B(1, []byte{255}))
	if _, e = m.Text(1, 20); e == nil {
		t.Fatal("UTF-8")
	}
}
func envelope(token, name string, code uint64, b []byte) []byte {
	return Join(B(1, S(1, token)), B(2, Join(V(1, code), S(2, "secret upstream body"))), B(3, Any(name, b)))
}
func TestResponseTokenAndType(t *testing.T) {
	b := envelope("fixture-private-instance", "X", 1, []byte{8, 1})
	p, token, e := Response(b, "X", "")
	if e != nil || token != "fixture-private-instance" || !bytes.Equal(p, []byte{8, 1}) {
		t.Fatal(e)
	}
	if _, _, e = Response(b, "Y", ""); e == nil {
		t.Fatal("type")
	}
	if _, _, e = Response(b, "X", "wrong"); AsError(e).Code != "StaleReference" {
		t.Fatal(e)
	}
}
func TestStatusTaxonomyAndRedaction(t *testing.T) {
	expect := map[uint64]string{0: "BackendFailed", 2: "Timeout", 3: "InvalidArgument", 4: "Unavailable", 5: "Unsupported", 6: "StaleReference", 7: "Unavailable", 8: "Unsupported", 99: "BackendFailed"}
	for n, code := range expect {
		_, _, err := Response(envelope("fixture-instance", "X", n, nil), "X", "")
		e := AsError(err)
		if e.Code != code || bytes.Contains([]byte(e.Message), []byte("secret")) {
			t.Fatalf("%d %+v", n, e)
		}
	}
}
func TestUncertaintyIsExplicit(t *testing.T) {
	e := E("Timeout", "bounded")
	u := Uncertain(e)
	if !e.OutcomeKnown || u.OutcomeKnown {
		t.Fatal("uncertainty mutated original")
	}
}
func TestSocketPathRestrictions(t *testing.T) {
	for _, p := range []string{"relative", "/tmp/../tmp/x", "/tmp/x\n"} {
		if _, e := SocketIdentityAt(p); e == nil {
			t.Fatal("bad path")
		}
	}
	tmp, e := os.MkdirTemp("", "kicad-socket-test-")
	if e != nil {
		t.Fatal(e)
	}
	defer os.RemoveAll(tmp)
	file := filepath.Join(tmp, "file")
	_ = os.WriteFile(file, []byte("x"), 0600)
	if _, e := SocketIdentityAt(file); e == nil {
		t.Fatal("regular file accepted")
	}
	link := filepath.Join(tmp, "link")
	_ = os.Symlink(file, link)
	if _, e := SocketIdentityAt(link); e == nil {
		t.Fatal("symlink accepted")
	}
}
func TestPrivateSocketAndStaleEndpoint(t *testing.T) {
	tmp, _ := os.MkdirTemp("", "kicad-socket-test-")
	defer os.RemoveAll(tmp)
	path := filepath.Join(tmp, "api.sock")
	ln, e := net.Listen("unix", path)
	if e != nil {
		t.Fatal(e)
	}
	defer ln.Close()
	if _, e = SocketIdentityAt(path); e != nil {
		t.Fatal(e)
	}
	_ = os.Chmod(tmp, 0755)
	if _, e = SocketIdentityAt(path); e == nil {
		t.Fatal("public parent accepted")
	}
}
func TestBadSPHandshake(t *testing.T) {
	tmp, _ := os.MkdirTemp("", "kicad-handshake-")
	defer os.RemoveAll(tmp)
	path := filepath.Join(tmp, "api.sock")
	ln, e := net.Listen("unix", path)
	if e != nil {
		t.Fatal(e)
	}
	defer ln.Close()
	done := make(chan struct{})
	go func() {
		defer close(done)
		c, e := ln.Accept()
		if e != nil {
			return
		}
		defer c.Close()
		var b [8]byte
		_, _ = c.Read(b[:])
		_, _ = c.Write(make([]byte, 8))
	}()
	if _, e = Dial(path, time.Second, 0); AsError(e).Code != "PluginProtocolError" {
		t.Fatal(e)
	}
	<-done
}
func FuzzWire(f *testing.F) {
	for _, b := range [][]byte{{}, {8, 1}, {10, 3, 97, 98, 99}, {255}, Join(V(1, ^uint64(0)), B(2, []byte("a")))} {
		f.Add(b)
	}
	f.Fuzz(func(t *testing.T, b []byte) {
		m, e := Parse(b)
		if e == nil {
			_, _ = m.One(1, 0)
			_, _ = m.Packed(2, 128)
			_, _ = m.Text(3, 256)
			_, _ = Replace(b, 1, V(1, 1))
		}
	})
}
func FuzzEnvelope(f *testing.F) {
	f.Add(envelope("fixture-instance", "X", 1, V(1, 3)))
	f.Add([]byte{255})
	f.Fuzz(func(t *testing.T, b []byte) { _, _, _ = Response(b, "X", "fixture-instance") })
}
func TestFixedWidthFields(t *testing.T) {
	var b [8]byte
	binary.LittleEndian.PutUint64(b[:], 7)
	m, e := Parse(Join([]byte{9}, b[:]))
	if e != nil || len(m) != 1 || len(m[0].Bytes) != 8 {
		t.Fatal(e)
	}
}
