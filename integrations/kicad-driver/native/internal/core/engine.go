// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"semwright-kicad-native/internal/wire"
	"strings"
	"time"
)

const common = "kiapi.common.commands."
const board = "kiapi.board.commands."
const maxRefs = 2048
const refTTL = 60 * time.Second

type reference struct {
	epoch, instance, document, uuid, kind, fingerprint, container string
	issued                                                        time.Time
}
type Engine struct {
	gate      chan struct{}
	cfg       Config
	channel   *wire.Channel
	selected  string
	token     string
	client    string
	epoch     string
	version   Version
	refs      map[string]reference
	caps      []Capability
	closed    bool
	tainted   bool
	restart   bool
	lastError string
	now       func() time.Time
}

func randomID() (string, error) {
	b := make([]byte, 16)
	if _, e := rand.Read(b); e != nil {
		return "", wire.E("Internal", "Cannot create unpredictable session identity")
	}
	return hex.EncodeToString(b), nil
}
func New(c Config) (*Engine, error) {
	epoch, e := randomID()
	if e != nil {
		return nil, e
	}
	engine := &Engine{gate: make(chan struct{}, 1), cfg: c, epoch: epoch, refs: map[string]reference{}, now: time.Now}
	selected := c.SelectedInstance
	if selected == "" && len(c.Instances) == 1 {
		selected = c.Instances[0].ID
	}
	if selected != "" {
		if e = engine.connect(selected); e != nil {
			engine.lastError = wire.AsError(e).Code
		}
	}
	engine.caps = Catalog(engine.channel != nil, engine.version.Supported, c.EnableMutations)
	return engine, nil
}
func (e *Engine) lock() error {
	select {
	case e.gate <- struct{}{}:
		if e.closed {
			<-e.gate
			return wire.E("Unavailable", "Driver is shut down")
		}
		return nil
	default:
		return wire.E("ResourceExhausted", "One KiCad operation is already in progress; bounded admission rejected this call")
	}
}
func (e *Engine) unlock() { <-e.gate }
func (e *Engine) invalidate() {
	if e.channel != nil {
		e.channel.Close()
		e.channel = nil
	}
	e.refs = map[string]reference{}
	epoch, err := randomID()
	if err != nil {
		e.closed = true
		e.epoch = ""
	} else {
		e.epoch = epoch
	}
}
func (e *Engine) connect(id string) error {
	var selected *Instance
	for i := range e.cfg.Instances {
		if e.cfg.Instances[i].ID == id {
			selected = &e.cfg.Instances[i]
			break
		}
	}
	if selected == nil {
		return wire.E("PermissionDenied", "Instance is outside the owner-configured allowlist")
	}
	e.invalidate()
	if e.closed {
		return wire.E("Internal", "Session identity generation failed")
	}
	e.selected = id
	e.token = selected.Token
	e.version = Version{}
	c, err := wire.Dial(selected.Socket, time.Duration(e.cfg.TimeoutMS)*time.Millisecond, selected.ExpectedPID)
	if err != nil {
		return err
	}
	e.channel = c
	e.client = "org.semwright.kicad-" + e.epoch
	raw, err := e.call(common+"GetVersion", common+"GetVersionResponse", nil, false)
	if err != nil {
		e.invalidate()
		return err
	}
	v, err := decodeVersion(raw)
	if err != nil {
		e.invalidate()
		return err
	}
	e.version = v
	e.lastError = ""
	return nil
}
func (e *Engine) call(request, response string, b []byte, mutation bool) ([]byte, error) {
	if e.channel == nil {
		return nil, wire.E("Unavailable", "Selected KiCad instance is disconnected; reconnect explicitly")
	}
	raw, err := e.channel.Exchange(wire.Request(e.token, e.client, request, b), mutation)
	if err != nil {
		e.invalidate()
		if mutation && !wire.AsError(err).OutcomeKnown {
			e.tainted = true
		}
		return nil, err
	}
	value, t, err := wire.Response(raw, response, e.token)
	if err != nil {
		code := wire.AsError(err).Code
		if code == "StaleReference" || code == "PluginProtocolError" || code == "ResourceExhausted" || code == "Timeout" {
			e.invalidate()
			if mutation {
				e.tainted = true
				return nil, wire.Uncertain(err)
			}
		}
		return nil, err
	}
	e.token = t
	return value, nil
}
func (e *Engine) status() map[string]any {
	state := "disconnected"
	if e.channel != nil {
		state = "connected"
	}
	return map[string]any{"driver_version": DriverVersion, "version": e.version, "instance_count": len(e.cfg.Instances), "selected_instance": e.selected, "connection_state": state, "generation": e.epoch, "capability_count": len(e.caps), "editor": "pcb", "reconciliation_required": e.tainted, "restart_required": e.restart, "last_error_code": e.lastError, "cached_observation": true}
}
func (e *Engine) egress(v any) (any, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, wire.E("BackendFailed", "Output encoding failed")
	}
	if len(b) > 900000 {
		return nil, wire.E("ResourceExhausted", "Projected result exceeds driver frame budget")
	}
	tokens := []string{e.token}
	for _, i := range e.cfg.Instances {
		tokens = append(tokens, i.Token)
	}
	for _, token := range tokens {
		if token == "" {
			continue
		}
		encoded, _ := json.Marshal(token)
		if len(encoded) > 2 && strings.Contains(string(b), string(encoded[1:len(encoded)-1])) {
			return nil, wire.E("PluginProtocolError", "Result contains session-private bytes and was withheld")
		}
	}
	return v, nil
}
func (e *Engine) Capabilities() ([]Capability, error) {
	if err := e.lock(); err != nil {
		return nil, err
	}
	defer e.unlock()
	return e.caps, nil
}
func (e *Engine) Health() (any, error) {
	if err := e.lock(); err != nil {
		return nil, err
	}
	defer e.unlock()
	return e.egress(e.status())
}
func (e *Engine) Close() { e.gate <- struct{}{}; defer e.unlock(); e.closed = true; e.invalidate() }
func (e *Engine) Execute(command, pin string, args map[string]any) (any, error) {
	if err := e.lock(); err != nil {
		return nil, err
	}
	defer e.unlock()
	var c *Capability
	for i := range e.caps {
		if e.caps[i].Descriptor.Name == command {
			c = &e.caps[i]
			break
		}
	}
	if c == nil {
		return nil, wire.E("Unsupported", "Command is outside the frozen capability catalog")
	}
	if !digestPattern.MatchString(pin) || pin != Digest(c.Descriptor) {
		return nil, wire.E("Conflict", "Command descriptor digest does not match the enumerated catalog")
	}
	if err := validateArgs(c.Descriptor.InputSchema, args); err != nil {
		return nil, err
	}
	result, err := e.execute(strings.TrimPrefix(command, Namespace), args, c.Descriptor.Risk != "read_only")
	if err != nil {
		e.lastError = wire.AsError(err).Code
		return nil, err
	}
	value, outputErr := e.egress(result)
	if outputErr != nil && c.Descriptor.Risk != "read_only" {
		return nil, e.uncertain("Mutation completed but its result could not be safely delivered")
	}
	return value, outputErr
}
func (e *Engine) documents() ([]Document, error) {
	raw, err := e.call(common+"GetOpenDocuments", common+"GetOpenDocumentsResponse", wire.V(1, 3), false)
	if err != nil {
		return nil, err
	}
	r := read(raw)
	out := []Document{}
	if r.err != nil {
		return nil, r.err
	}
	if len(r.m.All(1)) > 16 {
		return nil, wire.E("ResourceExhausted", "Too many open PCB documents")
	}
	seen := map[string]bool{}
	for _, f := range r.m.All(1) {
		if f.Kind != 2 {
			return nil, wire.E("PluginProtocolError", "Invalid document response field")
		}
		d, err := document(f.Bytes)
		if err != nil {
			return nil, err
		}
		if seen[d.ID] {
			return nil, wire.E("PluginProtocolError", "Duplicate open document identity")
		}
		seen[d.ID] = true
		out = append(out, d)
	}
	return out, nil
}
func (e *Engine) current() (Document, error) {
	docs, err := e.documents()
	if err != nil {
		return Document{}, err
	}
	if len(docs) == 0 {
		e.refs = map[string]reference{}
		return Document{}, wire.E("NotFound", "No PCB document is open")
	}
	if len(docs) > 1 {
		e.refs = map[string]reference{}
		out := wire.E("AmbiguousTarget", "Choose an unambiguous PCB editor instance before operating")
		for _, d := range docs {
			out.Candidates = append(out.Candidates, d.ID)
		}
		return Document{}, out
	}
	d := docs[0]
	for k, r := range e.refs {
		if r.document != d.ID {
			delete(e.refs, k)
		}
	}
	return d, nil
}
func (e *Engine) allowMutation(d Document) error {
	if !e.cfg.EnableMutations {
		return wire.E("PermissionDenied", "Owner disabled mutations")
	}
	if e.tainted {
		return wire.Uncertain(wire.E("Conflict", "Previous mutation outcome requires owner reconciliation before further mutations"))
	}
	for _, allowed := range e.cfg.AllowedDocuments {
		if allowed.BoardFilename == d.BoardFilename && allowed.ProjectPath == d.ProjectPath {
			return nil
		}
	}
	return wire.E("PermissionDenied", "Live PCB document is outside the exact owner mutation allowlist")
}
func (e *Engine) items(d Document, kind, container string) ([]Item, error) {
	k, ok := kindByName(kind)
	if !ok {
		return nil, wire.E("Unsupported", "Unsupported object kind")
	}
	raw, err := e.call(common+"GetItems", common+"GetItemsResponse", wire.Join(wire.B(1, header(d, container)), wire.B(2, wire.Uint(k.Code))), false)
	if err != nil {
		return nil, err
	}
	r := read(raw)
	h := r.child(1)
	returned, err := document(h.b(1))
	if err != nil {
		return nil, err
	}
	status := r.u(2)
	if r.err != nil {
		return nil, r.err
	}
	if h.err != nil {
		return nil, h.err
	}
	if returned.ID != d.ID {
		return nil, wire.E("StaleReference", "Item response belongs to another document")
	}
	if status != 1 {
		if status == 2 {
			return nil, wire.E("StaleReference", "Requested document is no longer open")
		}
		return nil, wire.E("BackendFailed", "KiCad item request was not successful")
	}
	fields := r.m.All(3)
	if len(fields) > 4096 {
		return nil, wire.E("ResourceExhausted", "Object count exceeds scan budget")
	}
	out := []Item{}
	seen := map[string]bool{}
	for _, f := range fields {
		if f.Kind != 2 {
			return nil, wire.E("PluginProtocolError", "Invalid object response field")
		}
		item, err := decodeItem(f.Bytes)
		if err != nil {
			return nil, err
		}
		if item.Kind != kind || seen[item.UUID] {
			return nil, wire.E("PluginProtocolError", "Unexpected or duplicate object identity")
		}
		seen[item.UUID] = true
		out = append(out, item)
	}
	// Observed absence invalidates old refs, even if the same UUID appears again later.
	for key, r := range e.refs {
		if r.document == d.ID && r.kind == kind && r.container == container && !seen[r.uuid] {
			delete(e.refs, key)
		}
	}
	return out, nil
}
func (e *Engine) issue(d Document, item Item, container string) (Item, error) {
	for key, r := range e.refs {
		if e.now().Sub(r.issued) > refTTL {
			delete(e.refs, key)
		}
	}
	if len(e.refs) >= maxRefs {
		return Item{}, wire.E("ResourceExhausted", "Reference capacity reached; wait for expiry or explicitly reconnect")
	}
	nonce, err := randomID()
	if err != nil {
		return Item{}, err
	}
	ref := "kc:" + e.epoch + ":" + nonce
	e.refs[ref] = reference{e.epoch, e.selected, d.ID, item.UUID, item.Kind, item.Fingerprint, container, e.now()}
	item.Ref = ref
	return item, nil
}
func (e *Engine) resolve(ref string) (Document, Item, reference, error) {
	if !refPattern.MatchString(ref) {
		return Document{}, Item{}, reference{}, wire.E("StaleReference", "Invalid object reference")
	}
	r, ok := e.refs[ref]
	if !ok || r.epoch != e.epoch || r.instance != e.selected || e.now().Sub(r.issued) > refTTL {
		delete(e.refs, ref)
		return Document{}, Item{}, reference{}, wire.E("StaleReference", "Object reference expired or belongs to another session")
	}
	d, err := e.current()
	if err != nil {
		return Document{}, Item{}, r, err
	}
	if r.document != d.ID {
		return Document{}, Item{}, r, wire.E("StaleReference", "Object reference belongs to another document")
	}
	items, err := e.items(d, r.kind, r.container)
	if err != nil {
		return Document{}, Item{}, r, err
	}
	for _, item := range items {
		if item.UUID == r.uuid {
			if item.Fingerprint != r.fingerprint {
				delete(e.refs, ref)
				return Document{}, Item{}, r, wire.E("StaleReference", "Object contents changed since reference issuance")
			}
			item.Ref = ref
			return d, item, r, nil
		}
	}
	delete(e.refs, ref)
	return Document{}, Item{}, r, wire.E("StaleReference", "Referenced object no longer exists")
}
func (e *Engine) listed(d Document, items []Item, container string, args map[string]any) (any, error) {
	offset, limit := int(asInt(args, "offset", 0)), int(asInt(args, "limit", 64))
	out := []Item{}
	end := offset + limit
	if end > len(items) {
		end = len(items)
	}
	for i := offset; i < end; i++ {
		item, err := e.issue(d, items[i], container)
		if err != nil {
			return nil, err
		}
		out = append(out, item)
	}
	return map[string]any{"document": d, "items": out, "total": len(items), "truncated": end < len(items)}, nil
}
