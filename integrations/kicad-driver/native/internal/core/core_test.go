// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"encoding/json"
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"semwright-kicad-native/internal/wire"
	"strings"
	"testing"
	"time"
)

func configuration() Config {
	return Config{SchemaVersion: 1, TimeoutMS: 100, Instances: []Instance{}, AllowedDocuments: []AllowedDocument{}}
}
func TestVersionBoundaries(t *testing.T) {
	for s, supported := range map[string]bool{"9.0.0": true, "10.0.6": true, "8.0.0": false, "11.0.0": false, "10.99.0": false, "10.0.6-dev": false, "10.0.6+build": false} {
		v, e := ParseVersion(s)
		if e != nil || v.Supported != supported {
			t.Fatalf("%s %+v %v", s, v, e)
		}
	}
	for _, s := range []string{"9", "a.b.c", "9999.0.0", "9.0.0\n", " 9.0.0", "9.0.-1"} {
		if _, e := ParseVersion(s); e == nil {
			t.Fatal(s)
		}
	}
}
func TestVersionWireMismatchDeclinesCatalog(t *testing.T) {
	v, e := decodeVersion(wire.B(1, wire.Join(wire.V(1, 11), wire.S(4, "10.0.6"))))
	if e != nil || v.Supported {
		t.Fatal(v, e)
	}
}
func TestStrictJSONRejectsDuplicatesAndUnknowns(t *testing.T) {
	var v struct {
		A int `json:"a"`
	}
	for _, s := range []string{`{"a":1,"a":2}`, `{"b":1}`, `{"a":1} {}`, `{"a":1e100}`, `[]`} {
		if e := StrictJSON([]byte(s), &v); e == nil {
			t.Fatal(s)
		}
	}
}
func TestStrictJSONBudgetAndMalformedUTF8(t *testing.T) {
	var v any
	for _, b := range [][]byte{[]byte(strings.Repeat("[", 40) + strings.Repeat("]", 40)), {34, 255, 34}, make([]byte, 1048577)} {
		if e := StrictJSON(b, &v); e == nil {
			t.Fatal("budget")
		}
	}
}
func TestConfigurationLimitsAndNoTokenDiagnostics(t *testing.T) {
	c := configuration()
	c.Instances = []Instance{{ID: "fixture", Socket: "/tmp/private/api.sock", Token: "a-private-fixture-token"}}
	b, _ := json.Marshal(c)
	if _, e := ParseConfig(b); e != nil {
		t.Fatal(e)
	}
	if strings.Contains(fmt.Sprintf("%v %#v", c, c.Instances[0]), "a-private") {
		t.Fatal("leak")
	}
	c.EnableMutations = true
	b, _ = json.Marshal(c)
	if _, e := ParseConfig(b); e == nil {
		t.Fatal("no allowlist")
	}
}
func TestConfigurationDuplicateInstanceAndPath(t *testing.T) {
	c := configuration()
	i := Instance{ID: "fixture", Socket: "/tmp/private/api.sock"}
	c.Instances = []Instance{i, i}
	b, _ := json.Marshal(c)
	if _, e := ParseConfig(b); e == nil {
		t.Fatal("duplicate")
	}
	c.Instances = []Instance{i}
	c.SelectedInstance = "other"
	b, _ = json.Marshal(c)
	if _, e := ParseConfig(b); e == nil {
		t.Fatal("selector")
	}
}
func TestReadConfigurationPermissionsAndSymlinks(t *testing.T) {
	tmp, _ := os.MkdirTemp("", "kicad-config-test-")
	defer os.RemoveAll(tmp)
	path := filepath.Join(tmp, "connection.json")
	b, _ := json.Marshal(configuration())
	_ = os.WriteFile(path, b, 0600)
	if _, e := ReadConfig(path); e != nil {
		t.Fatal(e)
	}
	_ = os.Chmod(path, 0644)
	if _, e := ReadConfig(path); e == nil {
		t.Fatal("mode")
	}
	link := filepath.Join(tmp, "link")
	_ = os.Symlink(path, link)
	if _, e := ReadConfig(link); e == nil {
		t.Fatal("symlink")
	}
}
func TestCatalogFilteringAndBudgets(t *testing.T) {
	for _, x := range []struct {
		connected, supported, mutation bool
		count                          int
	}{{false, false, false, 3}, {true, false, true, 5}, {true, true, false, 19}, {true, true, true, 23}} {
		caps := Catalog(x.connected, x.supported, x.mutation)
		if len(caps) != x.count {
			t.Fatal(len(caps))
		}
		for _, c := range caps {
			d := c.Descriptor
			if !strings.HasPrefix(d.Name, Namespace) || len(d.Name) > 128 || len(Digest(d)) != 64 || d.Backends[0] != ProviderID || d.Requires[0] != ProviderID {
				t.Fatal(d.Name)
			}
			if d.Risk != "read_only" && (!d.InteractiveConsent || len(d.Requires) != 2 || d.DryRun) {
				t.Fatal("mutation metadata")
			}
			if strings.Contains(d.Name, "raw") || strings.Contains(d.Name, "shell") {
				t.Fatal("escape hatch")
			}
		}
	}
}
func TestDescriptorPinAndSchemaBeforeDispatch(t *testing.T) {
	e, err := New(configuration())
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()
	c := e.caps[2]
	for _, entry := range e.caps {
		if entry.Descriptor.Name == Namespace+"status" {
			c = entry
		}
	}
	if _, err = e.Execute(c.Descriptor.Name, strings.Repeat("0", 64), map[string]any{}); wire.AsError(err).Code != "Conflict" {
		t.Fatal(err)
	}
	if _, err = e.Execute(c.Descriptor.Name, Digest(c.Descriptor), map[string]any{"unexpected": true}); wire.AsError(err).Code != "InvalidArgument" {
		t.Fatal(err)
	}
	if _, err = e.Execute(c.Descriptor.Name, Digest(c.Descriptor), map[string]any{}); err != nil {
		t.Fatal(err)
	}
}
func TestIntegerSchemaRejectsFloatingPointAndOverflows(t *testing.T) {
	s := object(map[string]any{"x": integer(-3, 3)}, "x")
	for _, v := range []any{json.Number("4"), json.Number("1.0"), json.Number("1e0"), true, "1", json.Number("9223372036854775808")} {
		if e := validateArgs(s, map[string]any{"x": v}); e == nil {
			t.Fatalf("accepted %v", v)
		}
	}
}
func TestTranslationProperty(t *testing.T) {
	r := rand.New(rand.NewSource(81023))
	for i := 0; i < 10000; i++ {
		p := Point{r.Int63n(2000000000) - 1000000000, r.Int63n(2000000000) - 1000000000}
		dx, dy := r.Int63n(200000000)-100000000, r.Int63n(200000000)-100000000
		q, e := Translate(p, dx, dy)
		if e != nil {
			t.Fatal(e)
		}
		back, e := Translate(q, -dx, -dy)
		if e != nil || back != p {
			t.Fatal("inverse")
		}
	}
}
func TestTranslationRefusesUnboundedGeometry(t *testing.T) {
	for _, p := range []Point{{2000000001, 0}, {-2000000001, 0}, {2000000000, 0}} {
		if _, e := Translate(p, 1, 0); e == nil {
			t.Fatal(p)
		}
	}
}
func TestReferenceEpochTTLAndCap(t *testing.T) {
	e, _ := New(configuration())
	defer e.Close()
	now := time.Unix(100, 0)
	e.now = func() time.Time { return now }
	d := Document{ID: strings.Repeat("d", 64)}
	item := Item{UUID: "00000000-0000-4000-8000-000000000001", Kind: "track", Fingerprint: strings.Repeat("a", 64)}
	issued, err := e.issue(d, item, "")
	if err != nil || !refPattern.MatchString(issued.Ref) {
		t.Fatal(err)
	}
	now = now.Add(61 * time.Second)
	if _, _, _, err = e.resolve(issued.Ref); wire.AsError(err).Code != "StaleReference" {
		t.Fatal(err)
	}
	for i := 0; i < maxRefs; i++ {
		e.refs[fmt.Sprint(i)] = reference{issued: now}
	}
	if _, err = e.issue(d, item, ""); wire.AsError(err).Code != "ResourceExhausted" {
		t.Fatal(err)
	}
}
func TestAdmissionBoundRejectsConcurrentWork(t *testing.T) {
	e, _ := New(configuration())
	defer e.Close()
	if err := e.lock(); err != nil {
		t.Fatal(err)
	}
	if _, err := e.Health(); wire.AsError(err).Code != "ResourceExhausted" {
		t.Fatal(err)
	}
	e.unlock()
}
func TestMutationRequiresExactDocumentAndQuarantine(t *testing.T) {
	c := configuration()
	c.EnableMutations = true
	c.AllowedDocuments = []AllowedDocument{{BoardFilename: "a.kicad_pcb", ProjectPath: "/fixture"}}
	e, _ := New(c)
	defer e.Close()
	d := Document{BoardFilename: "a.kicad_pcb", ProjectPath: "/fixture"}
	if err := e.allowMutation(d); err != nil {
		t.Fatal(err)
	}
	d.ProjectPath = "/elsewhere"
	if err := e.allowMutation(d); err == nil {
		t.Fatal("cross project")
	}
	e.tainted = true
	d.ProjectPath = "/fixture"
	if err := e.allowMutation(d); wire.AsError(err).OutcomeKnown {
		t.Fatal("quarantine")
	}
}
func TestEgressNeverReflectsToken(t *testing.T) {
	e, _ := New(configuration())
	defer e.Close()
	e.token = "test-private-instance-token"
	if _, err := e.egress(map[string]any{"label": e.token}); err == nil {
		t.Fatal("token reflection")
	}
	if _, err := e.egress(map[string]any{"label": "untrusted instructions"}); err != nil {
		t.Fatal(err)
	}
}
func TestPaginationNeverReturnsBareIndexRef(t *testing.T) {
	e, _ := New(configuration())
	defer e.Close()
	d := Document{ID: strings.Repeat("d", 64)}
	items := []Item{{UUID: "00000000-0000-4000-8000-000000000001", Kind: "zone"}}
	result, err := e.listed(d, items, "", map[string]any{})
	if err != nil {
		t.Fatal(err)
	}
	if !refPattern.MatchString(result.(map[string]any)["items"].([]Item)[0].Ref) {
		t.Fatal("ref")
	}
}
func FuzzVersionReference(f *testing.F) {
	for _, s := range []string{"9.0.0", "11.0.0-dev", "kc:" + strings.Repeat("a", 32) + ":" + strings.Repeat("b", 32), "\x00"} {
		f.Add(s)
	}
	f.Fuzz(func(t *testing.T, s string) {
		_, _ = ParseVersion(s)
		_ = refPattern.MatchString(s)
		_ = uuidPattern.MatchString(s)
	})
}
func FuzzConfig(f *testing.F) {
	b, _ := json.Marshal(configuration())
	f.Add(b)
	f.Add([]byte(`{"schema_version":1,"schema_version":2}`))
	f.Fuzz(func(t *testing.T, b []byte) { _, _ = ParseConfig(b) })
}
func FuzzObjectBoundary(f *testing.F) {
	f.Add(wire.Any("kiapi.board.types.Track", wire.B(1, wire.S(1, "00000000-0000-4000-8000-000000000001"))))
	f.Fuzz(func(t *testing.T, b []byte) { _, _ = decodeItem(b); _, _ = document(b) })
}
