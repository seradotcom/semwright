// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"math"
	"semwright-kicad-native/internal/wire"
)

type reader struct {
	m   wire.Message
	err error
}

func read(b []byte) *reader { m, e := wire.Parse(b); return &reader{m, e} }
func (r *reader) u(n uint32) uint64 {
	if r.err != nil {
		return 0
	}
	v, e := r.m.Number(n)
	r.err = e
	return v
}
func (r *reader) b(n uint32) []byte {
	if r.err != nil {
		return nil
	}
	v, e := r.m.Data(n)
	r.err = e
	return v
}
func (r *reader) s(n uint32, max int) string {
	if r.err != nil {
		return ""
	}
	v, e := r.m.Text(n, max)
	r.err = e
	return v
}
func (r *reader) child(n uint32) *reader {
	b := r.b(n)
	if r.err != nil {
		return &reader{err: r.err}
	}
	return read(b)
}
func digest(b []byte) string { s := sha256.Sum256(b); return hex.EncodeToString(s[:]) }

type Point struct {
	X int64 `json:"x_nm"`
	Y int64 `json:"y_nm"`
}

func point(b []byte) (Point, error) {
	r := read(b)
	p := Point{int64(r.u(1)), int64(r.u(2))}
	return p, r.err
}
func pointBytes(p Point) []byte { return wire.Join(wire.V(1, uint64(p.X)), wire.V(2, uint64(p.Y))) }

// Distances are integral nanometers. The mutation envelope stays inside KiCad's signed 32-bit working range.
func Translate(p Point, dx, dy int64) (Point, error) {
	const bound int64 = 2_000_000_000
	const delta int64 = 100_000_000
	if dx < -delta || dx > delta || dy < -delta || dy > delta || p.X < -bound || p.X > bound || p.Y < -bound || p.Y > bound {
		return Point{}, wire.E("InvalidArgument", "Geometry outside safe nanometer bounds")
	}
	q := Point{p.X + dx, p.Y + dy}
	if q.X < -bound || q.X > bound || q.Y < -bound || q.Y > bound {
		return Point{}, wire.E("InvalidArgument", "Move exceeds bounded board coordinates")
	}
	return q, nil
}

type Document struct {
	ID            string `json:"document_id"`
	BoardFilename string `json:"board_filename"`
	ProjectName   string `json:"project_name"`
	ProjectPath   string `json:"project_path"`
	Untrusted     bool   `json:"untrusted_content"`
	Raw           []byte `json:"-"`
}

func document(b []byte) (Document, error) {
	r := read(b)
	typ := r.u(1)
	name := r.s(4, 256)
	p := r.child(5)
	project := p.s(1, 256)
	path := p.s(2, 4096)
	if r.err != nil {
		return Document{}, r.err
	}
	if p.err != nil {
		return Document{}, p.err
	}
	if typ != 3 || name == "" {
		return Document{}, wire.E("Unsupported", "Only explicit PCB document specifiers are supported")
	}
	// Hash the complete specifier, retaining unfamiliar fields conservatively.
	return Document{digest(b), name, project, path, true, append([]byte(nil), b...)}, nil
}
func header(d Document, container string) []byte {
	b := wire.B(1, d.Raw)
	if container != "" {
		b = wire.Join(b, wire.B(2, wire.S(1, container)))
	}
	return b
}

type Kind struct {
	Code  uint64
	Name  string
	Proto string
}

var Kinds = []Kind{{1, "footprint", "FootprintInstance"}, {2, "pad", "Pad"}, {11, "track", "Track"}, {12, "via", "Via"}, {16, "zone", "Zone"}}

func kindByName(s string) (Kind, bool) {
	for _, k := range Kinds {
		if k.Name == s {
			return k, true
		}
	}
	return Kind{}, false
}
func kindByProto(s string) (Kind, bool) {
	for _, k := range Kinds {
		if "kiapi.board.types."+k.Proto == s {
			return k, true
		}
	}
	return Kind{}, false
}

type Item struct {
	UUID            string   `json:"object_uuid"`
	Kind            string   `json:"kind"`
	Ref             string   `json:"object_ref"`
	Fingerprint     string   `json:"fingerprint"`
	Untrusted       bool     `json:"untrusted_content"`
	Position        *Point   `json:"position,omitempty"`
	End             *Point   `json:"end,omitempty"`
	CoordinateSpace string   `json:"coordinate_space,omitempty"`
	Angle           *float64 `json:"angle_degrees,omitempty"`
	Locked          *bool    `json:"locked,omitempty"`
	Label           *string  `json:"label,omitempty"`
	Net             *string  `json:"net,omitempty"`
	Layer           *uint64  `json:"layer,omitempty"`
	Width           *int64   `json:"width_nm,omitempty"`
	Raw             []byte   `json:"-"`
	Any             []byte   `json:"-"`
}

func decodeItem(b []byte) (Item, error) {
	a := read(b)
	url := a.s(1, 256)
	raw := a.b(2)
	if a.err != nil {
		return Item{}, a.err
	}
	if len(url) < len(wire.Prefix) || url[:len(wire.Prefix)] != wire.Prefix {
		return Item{}, wire.E("PluginProtocolError", "Invalid item type URL")
	}
	k, ok := kindByProto(url[len(wire.Prefix):])
	if !ok {
		return Item{}, wire.E("Unsupported", "Item type is outside the curated projection")
	}
	r := read(raw)
	id := r.child(1)
	uuid := id.s(1, 36)
	if id.err != nil {
		return Item{}, id.err
	}
	if !uuidPattern.MatchString(uuid) {
		return Item{}, wire.E("PluginProtocolError", "Invalid KiCad object UUID")
	}
	out := Item{UUID: uuid, Kind: k.Name, Fingerprint: digest(raw), Untrusted: true, Raw: append([]byte(nil), raw...), Any: append([]byte(nil), b...)}
	posTag, lockTag, netTag, layerTag := uint32(0), uint32(0), uint32(0), uint32(0)
	switch k.Name {
	case "footprint":
		posTag = 2
		lockTag = 5
		layerTag = 4
		angle := r.child(3)
		f, e := angle.m.One(1, 1)
		if e != nil {
			return Item{}, e
		}
		v := 0.0
		if len(f.Bytes) == 8 {
			v = math.Float64frombits(binary.LittleEndian.Uint64(f.Bytes))
		}
		if math.IsNaN(v) || math.IsInf(v, 0) {
			return Item{}, wire.E("PluginProtocolError", "Invalid footprint angle")
		}
		out.Angle = &v
		field := r.child(7)
		bt := field.child(3)
		tx := bt.child(2)
		label := tx.s(5, 256)
		for _, v := range []*reader{angle, field, bt, tx} {
			if v.err != nil {
				return Item{}, v.err
			}
		}
		out.Label = &label
	case "pad":
		posTag = 7
		lockTag = 2
		netTag = 4
		label := r.s(3, 256)
		out.Label = &label
		out.CoordinateSpace = "parent-footprint"
	case "track":
		posTag = 2
		lockTag = 5
		netTag = 7
		layerTag = 6
		p, e := point(r.b(3))
		if e != nil {
			return Item{}, e
		}
		out.End = &p
		w := r.child(4)
		width := int64(w.u(1))
		if w.err != nil {
			return Item{}, w.err
		}
		out.Width = &width
	case "via":
		posTag = 2
		lockTag = 4
		netTag = 5
	case "zone":
		label := r.s(5, 256)
		out.Label = &label
	}
	if posTag != 0 {
		p, e := point(r.b(posTag))
		if e != nil {
			return Item{}, e
		}
		out.Position = &p
		if out.CoordinateSpace == "" {
			out.CoordinateSpace = "board"
		}
	}
	if lockTag != 0 {
		lock := r.u(lockTag)
		if lock != 1 && lock != 2 {
			return Item{}, wire.E("PluginProtocolError", "Missing explicit object lock state")
		}
		v := lock == 2
		out.Locked = &v
	}
	if layerTag != 0 {
		v := r.u(layerTag)
		if v > 10000 {
			return Item{}, wire.E("PluginProtocolError", "Layer identifier exceeds supported budget")
		}
		out.Layer = &v
	}
	if netTag != 0 {
		n := r.child(netTag)
		v := n.s(2, 256)
		if n.err != nil {
			return Item{}, n.err
		}
		out.Net = &v
	}
	if r.err != nil {
		return Item{}, r.err
	}
	return out, nil
}
func moved(item Item, dx, dy int64) ([]byte, error) {
	if item.Kind != "track" && item.Kind != "via" {
		return nil, wire.E("Unsupported", "Geometry mutation is restricted to tracks and vias")
	}
	if item.Locked == nil || *item.Locked {
		return nil, wire.E("PermissionDenied", "Locked or unknown-lock object cannot be moved")
	}
	if item.Position == nil {
		return nil, wire.E("PluginProtocolError", "Item position missing")
	}
	p, e := Translate(*item.Position, dx, dy)
	if e != nil {
		return nil, e
	}
	out, e := wire.Replace(item.Raw, 2, wire.B(2, pointBytes(p)))
	if e != nil {
		return nil, e
	}
	if item.Kind == "track" {
		if item.End == nil {
			return nil, wire.E("PluginProtocolError", "Track endpoint missing")
		}
		q, e := Translate(*item.End, dx, dy)
		if e != nil {
			return nil, e
		}
		out, e = wire.Replace(out, 3, wire.B(3, pointBytes(q)))
		if e != nil {
			return nil, e
		}
	}
	k, _ := kindByName(item.Kind)
	return wire.Any("kiapi.board.types."+k.Proto, out), nil
}
