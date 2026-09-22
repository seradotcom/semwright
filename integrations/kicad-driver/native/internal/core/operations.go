// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"semwright-kicad-native/internal/wire"
	"sort"
	"strings"
)

func (e *Engine) execute(op string, args map[string]any, mutation bool) (any, error) {
	switch op {
	case "status":
		return e.status(), nil
	case "instances.list":
		rows := []map[string]any{}
		for _, i := range e.cfg.Instances {
			_, err := wire.SocketIdentityAt(i.Socket)
			code := ""
			if err != nil {
				code = wire.AsError(err).Code
			}
			rows = append(rows, map[string]any{"id": i.ID, "socket_present": err == nil, "selected": i.ID == e.selected, "inspection_error_code": code})
		}
		return map[string]any{"instances": rows}, nil
	case "instance.reconnect":
		// Frozen v1 catalog cannot gain capabilities. A changed surface requires a host restart.
		if err := e.connect(args["instance_id"].(string)); err != nil {
			return nil, err
		}
		e.restart = Digest(Catalog(true, e.version.Supported, e.cfg.EnableMutations)) != Digest(e.caps)
		return e.status(), nil
	}
	if e.channel == nil {
		return nil, wire.E("Unavailable", "Selected KiCad instance is disconnected")
	}
	switch op {
	case "version":
		raw, err := e.call(common+"GetVersion", common+"GetVersionResponse", nil, false)
		if err != nil {
			return nil, err
		}
		v, err := decodeVersion(raw)
		if err != nil {
			return nil, err
		}
		if v != e.version {
			e.invalidate()
			e.restart = true
			return nil, wire.E("StaleReference", "KiCad version changed; restart the driver")
		}
		return v, nil
	case "instance.inspect":
		return map[string]any{"id": e.selected, "generation": e.epoch, "peer_uid": e.channel.Peer.UID, "peer_pid": e.channel.Peer.PID, "version": e.version}, nil
	}
	if !e.version.Supported || e.restart {
		return nil, wire.E("Unsupported", "PCB operations require a supported, frozen startup catalog")
	}
	if mutation && e.tainted {
		return nil, wire.Uncertain(wire.E("Conflict", "Uncertain mutation requires owner reconciliation and driver restart"))
	}
	if op == "document.list" {
		d, err := e.documents()
		return map[string]any{"documents": d}, err
	}
	if op == "footprint.inspect" || op == "pads.list" || op == "track.move" || op == "via.move" || op == "selection.add" {
		key := "object_ref"
		if op == "pads.list" {
			key = "footprint_ref"
		}
		d, item, ref, err := e.resolve(args[key].(string))
		if err != nil {
			return nil, err
		}
		switch op {
		case "footprint.inspect", "pads.list":
			if item.Kind != "footprint" {
				return nil, wire.E("InvalidArgument", "This operation requires a footprint reference")
			}
			if op == "footprint.inspect" {
				return map[string]any{"document": d, "item": item}, nil
			}
			items, err := e.items(d, "pad", item.UUID)
			if err != nil {
				return nil, err
			}
			return e.listed(d, items, item.UUID, args)
		case "track.move", "via.move":
			if item.Kind != strings.TrimSuffix(op, ".move") {
				return nil, wire.E("InvalidArgument", "Object reference kind does not match the requested mutation")
			}
			if err = e.allowMutation(d); err != nil {
				return nil, err
			}
			return e.move(d, item, ref, args)
		case "selection.add":
			if err = e.allowMutation(d); err != nil {
				return nil, err
			}
			raw, err := e.call(common+"AddToSelection", common+"SelectionResponse", wire.Join(wire.B(1, header(d, ref.container)), wire.B(2, wire.S(1, item.UUID))), true)
			if err != nil {
				return nil, err
			}
			// Other selected kinds are legal. Validate envelopes/identities, not their full geometry.
			r := read(raw)
			found := false
			if len(r.m.All(1)) > 4096 {
				return nil, e.uncertain("Selection confirmation exceeds item budget")
			}
			for _, f := range r.m.All(1) {
				if f.Kind != 2 {
					return nil, e.uncertain("Invalid selection confirmation")
				}
				a := read(f.Bytes)
				body := a.child(2)
				id := body.child(1)
				uuid := id.s(1, 36)
				if a.err != nil || body.err != nil || id.err != nil || !uuidPattern.MatchString(uuid) {
					return nil, e.uncertain("Malformed selection confirmation")
				}
				if uuid == item.UUID {
					found = true
				}
			}
			if r.err != nil || !found {
				return nil, e.uncertain("Selected object was not confirmed")
			}
			return map[string]any{"document_id": d.ID, "selection_changed": true}, nil
		}
	}
	d, err := e.current()
	if err != nil {
		return nil, err
	}
	switch op {
	case "document.current", "project.inspect":
		return d, nil
	case "board.summary":
		out := map[string]any{"document": d, "complete_board_inventory": false}
		for _, pair := range [][2]string{{"footprints", "footprint"}, {"tracks", "track"}, {"vias", "via"}, {"zones", "zone"}} {
			items, err := e.items(d, pair[1], "")
			if err != nil {
				return nil, err
			}
			out[pair[0]] = len(items)
		}
		return out, nil
	case "board.items.list", "footprints.list", "tracks.list", "vias.list", "zones.list":
		kind := map[string]string{"footprints.list": "footprint", "tracks.list": "track", "vias.list": "via", "zones.list": "zone"}[op]
		if op == "board.items.list" {
			kind = args["kind"].(string)
		}
		items, err := e.items(d, kind, "")
		if err != nil {
			return nil, err
		}
		return e.listed(d, items, "", args)
	case "nets.list":
		raw, err := e.call(board+"GetNets", board+"NetsResponse", wire.B(1, d.Raw), false)
		if err != nil {
			return nil, err
		}
		r := read(raw)
		fields := r.m.All(1)
		if r.err != nil {
			return nil, r.err
		}
		if len(fields) > 4096 {
			return nil, wire.E("ResourceExhausted", "Net scan exceeds budget")
		}
		names := []string{}
		for _, f := range fields {
			if f.Kind != 2 {
				return nil, wire.E("PluginProtocolError", "Invalid net field")
			}
			n := read(f.Bytes)
			name := n.s(2, 256)
			if n.err != nil {
				return nil, n.err
			}
			names = append(names, name)
		}
		sort.Strings(names)
		offset, limit := int(asInt(args, "offset", 0)), int(asInt(args, "limit", 64))
		end := offset + limit
		if end > len(names) {
			end = len(names)
		}
		out := []map[string]any{}
		for i := offset; i < end; i++ {
			out = append(out, map[string]any{"name": names[i], "untrusted_content": true})
		}
		return map[string]any{"document": d, "nets": out, "total": len(names), "truncated": end < len(names)}, nil
	case "layers.list":
		raw, err := e.call(board+"GetBoardEnabledLayers", board+"BoardEnabledLayersResponse", wire.B(1, d.Raw), false)
		if err != nil {
			return nil, err
		}
		r := read(raw)
		copper := r.u(1)
		layers, err := r.m.Packed(2, 128)
		if r.err != nil {
			return nil, r.err
		}
		if err != nil {
			return nil, err
		}
		if copper > 128 {
			return nil, wire.E("PluginProtocolError", "Invalid copper layer count")
		}
		for _, v := range layers {
			if v > 10000 {
				return nil, wire.E("PluginProtocolError", "Layer identifier exceeds supported budget")
			}
		}
		return map[string]any{"document": d, "copper_layer_count": copper, "layers": layers}, nil
	case "selection.inspect":
		packed := wire.Join(wire.Uint(1), wire.Uint(11), wire.Uint(12), wire.Uint(16))
		raw, err := e.call(common+"GetSelection", common+"SelectionResponse", wire.Join(wire.B(1, header(d, "")), wire.B(2, packed)), false)
		if err != nil {
			return nil, err
		}
		r := read(raw)
		if r.err != nil {
			return nil, r.err
		}
		if len(r.m.All(1)) > 4096 {
			return nil, wire.E("ResourceExhausted", "Selection exceeds scan budget")
		}
		items := []Item{}
		seen := map[string]bool{}
		for _, f := range r.m.All(1) {
			if f.Kind != 2 {
				return nil, wire.E("PluginProtocolError", "Invalid selection item")
			}
			item, err := decodeItem(f.Bytes)
			if err != nil {
				return nil, err
			}
			if item.Kind == "pad" || seen[item.UUID] {
				return nil, wire.E("PluginProtocolError", "Invalid or duplicate selection object")
			}
			seen[item.UUID] = true
			items = append(items, item)
		}
		return e.listed(d, items, "", args)
	case "selection.clear":
		if args["document_id"] != d.ID {
			return nil, wire.E("StaleReference", "Selection clear requires the current document identity")
		}
		if err = e.allowMutation(d); err != nil {
			return nil, err
		}
		raw, err := e.call(common+"ClearSelection", "google.protobuf.Empty", wire.B(1, header(d, "")), true)
		if err != nil {
			return nil, err
		}
		if _, err = wire.Parse(raw); err != nil {
			return nil, e.uncertain("Invalid selection-clear confirmation")
		}
		return map[string]any{"document_id": d.ID, "selection_changed": true}, nil
	}
	return nil, wire.E("Unsupported", "Curated operation has no implementation")
}
func (e *Engine) uncertain(message string) error {
	e.tainted = true
	e.invalidate()
	return wire.Uncertain(wire.E("BackendFailed", message))
}
func (e *Engine) endCommit(id string, action uint64) error {
	raw, err := e.call(common+"EndCommit", common+"EndCommitResponse", wire.Join(wire.B(1, wire.S(1, id)), wire.V(2, action), wire.S(3, "Semwright bounded item move")), true)
	if err != nil {
		return err
	}
	if _, err = wire.Parse(raw); err != nil {
		return e.uncertain("Invalid commit confirmation")
	}
	return nil
}
func (e *Engine) dropCommit(id string, original error) error {
	if e.channel != nil {
		if err := e.endCommit(id, 2); err == nil {
			return wire.E(wire.AsError(original).Code, "Bounded operation failed; KiCad acknowledged dropping the staged commit")
		}
	}
	return e.uncertain("Staged KiCad commit could not be reconciled; inspect the disposable board before further work")
}
func (e *Engine) move(d Document, item Item, ref reference, args map[string]any) (any, error) {
	changed, err := moved(item, asInt(args, "dx_nm", 0), asInt(args, "dy_nm", 0))
	if err != nil {
		return nil, err
	}
	wanted, err := decodeItem(changed)
	if err != nil {
		return nil, err
	}
	raw, err := e.call(common+"BeginCommit", common+"BeginCommitResponse", nil, true)
	if err != nil {
		return nil, err
	}
	r := read(raw)
	idReader := r.child(1)
	commitID := idReader.s(1, 36)
	if r.err != nil || idReader.err != nil || !uuidPattern.MatchString(commitID) {
		return nil, e.dropCommit("", wire.E("PluginProtocolError", "Invalid commit identity"))
	}
	// Re-read under our pending commit. This narrows but cannot eliminate the GUI/CAS race.
	check, current, _, err := e.resolve(item.Ref)
	if err != nil {
		return nil, e.dropCommit(commitID, err)
	}
	if check.ID != d.ID || current.Fingerprint != item.Fingerprint {
		return nil, e.dropCommit(commitID, wire.E("StaleReference", "Object changed before update"))
	}
	raw, err = e.call(common+"UpdateItems", common+"UpdateItemsResponse", wire.Join(wire.B(1, header(d, ref.container)), wire.B(2, changed)), true)
	if err != nil {
		return nil, e.dropCommit(commitID, err)
	}
	r = read(raw)
	h := r.child(1)
	returned, docErr := document(h.b(1))
	status := r.u(2)
	fields := r.m.All(3)
	if docErr != nil || r.err != nil || h.err != nil || returned.ID != d.ID || status != 1 || len(fields) != 1 || fields[0].Kind != 2 {
		return nil, e.dropCommit(commitID, wire.E("PluginProtocolError", "Update response does not identify the scoped document and object"))
	}
	update := read(fields[0].Bytes)
	st := update.child(1)
	code := st.u(1)
	if update.err != nil || st.err != nil || code != 1 {
		return nil, e.dropCommit(commitID, wire.E("BackendFailed", "KiCad rejected the bounded item update"))
	}
	acknowledged, err := decodeItem(update.b(2))
	if err != nil || update.err != nil || acknowledged.UUID != item.UUID || acknowledged.Kind != item.Kind {
		return nil, e.dropCommit(commitID, wire.E("PluginProtocolError", "KiCad did not acknowledge the expected object"))
	}
	if err = e.endCommit(commitID, 1); err != nil {
		return nil, e.dropCommit(commitID, err)
	}
	// Commit acknowledgement is not postcondition evidence. Observe fresh geometry.
	postDoc, err := e.current()
	if err != nil || postDoc.ID != d.ID {
		return nil, e.uncertain("Commit acknowledged but current document could not be revalidated")
	}
	rows, err := e.items(d, item.Kind, ref.container)
	if err != nil {
		return nil, e.uncertain("Commit acknowledged but postcondition observation failed")
	}
	for _, post := range rows {
		if post.UUID == item.UUID {
			if post.Position == nil || wanted.Position == nil || *post.Position != *wanted.Position || (item.Kind == "track" && (post.End == nil || wanted.End == nil || *post.End != *wanted.End)) {
				return nil, e.uncertain("Commit acknowledged but expected geometry was not observed")
			}
			for key, r := range e.refs {
				if r.document == d.ID && r.uuid == item.UUID {
					delete(e.refs, key)
				}
			}
			post, err = e.issue(d, post, ref.container)
			if err != nil {
				return nil, e.uncertain("Mutation completed but result reference could not be issued")
			}
			return map[string]any{"document": d, "item": post, "committed": true, "save_requested": false}, nil
		}
	}
	return nil, e.uncertain("Commit acknowledged but mutated object disappeared")
}
