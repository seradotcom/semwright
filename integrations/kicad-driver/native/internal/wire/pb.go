// SPDX-License-Identifier: GPL-3.0-or-later
// A bounded protobuf wire reader, not a reflection-based raw request API.
package wire

import (
	"encoding/binary"
	"unicode/utf8"
)

const MaxMessage = 1 << 20
const MaxFields = 16384

type Field struct {
	Number uint32
	Kind   byte
	Bytes  []byte
	Uint   uint64
	Raw    []byte
}
type Message []Field

func varint(b []byte) (uint64, int, error) {
	var v uint64
	for i := 0; i < 10; i++ {
		if i >= len(b) {
			return 0, 0, E("PluginProtocolError", "Truncated protobuf integer")
		}
		c := b[i]
		if i == 9 && c > 1 {
			return 0, 0, E("PluginProtocolError", "Protobuf integer overflow")
		}
		v |= uint64(c&127) << uint(7*i)
		if c < 128 {
			return v, i + 1, nil
		}
	}
	return 0, 0, E("PluginProtocolError", "Invalid protobuf integer")
}
func Parse(b []byte) (Message, error) {
	if len(b) > MaxMessage {
		return nil, E("ResourceExhausted", "Protobuf message exceeds byte budget")
	}
	out := make(Message, 0, 8)
	for len(b) > 0 {
		if len(out) >= MaxFields {
			return nil, E("ResourceExhausted", "Protobuf field budget exceeded")
		}
		start := b
		tag, n, e := varint(b)
		if e != nil {
			return nil, e
		}
		b = b[n:]
		number := tag >> 3
		kind := byte(tag & 7)
		if number == 0 || number > 536870911 {
			return nil, E("PluginProtocolError", "Invalid protobuf tag")
		}
		f := Field{Number: uint32(number), Kind: kind}
		switch kind {
		case 0:
			v, n, e := varint(b)
			if e != nil {
				return nil, e
			}
			f.Uint = v
			b = b[n:]
		case 1:
			if len(b) < 8 {
				return nil, E("PluginProtocolError", "Truncated fixed64")
			}
			f.Bytes = b[:8]
			b = b[8:]
		case 2:
			size, n, e := varint(b)
			if e != nil {
				return nil, e
			}
			b = b[n:]
			if size > uint64(len(b)) {
				return nil, E("PluginProtocolError", "Truncated protobuf field")
			}
			f.Bytes = b[:int(size)]
			b = b[int(size):]
		case 5:
			if len(b) < 4 {
				return nil, E("PluginProtocolError", "Truncated fixed32")
			}
			f.Bytes = b[:4]
			b = b[4:]
		default:
			return nil, E("PluginProtocolError", "Unsupported protobuf wire kind")
		}
		f.Raw = start[:len(start)-len(b)]
		out = append(out, f)
	}
	return out, nil
}
func (m Message) All(n uint32) []Field {
	out := []Field{}
	for _, f := range m {
		if f.Number == n {
			out = append(out, f)
		}
	}
	return out
}
func (m Message) One(n uint32, kind byte) (Field, error) {
	var out Field
	found := false
	for _, f := range m {
		if f.Number == n {
			if found || f.Kind != kind {
				return out, E("PluginProtocolError", "Repeated or incorrectly typed singular protobuf field")
			}
			out = f
			found = true
		}
	}
	return out, nil
}
func (m Message) Data(n uint32) ([]byte, error) { f, e := m.One(n, 2); return f.Bytes, e }
func (m Message) Child(n uint32) (Message, error) {
	b, e := m.Data(n)
	if e != nil {
		return nil, e
	}
	return Parse(b)
}
func (m Message) Number(n uint32) (uint64, error) { f, e := m.One(n, 0); return f.Uint, e }
func (m Message) Text(n uint32, max int) (string, error) {
	b, e := m.Data(n)
	if e != nil {
		return "", e
	}
	if len(b) > max || !utf8.Valid(b) {
		return "", E("PluginProtocolError", "Invalid or oversized upstream string")
	}
	return string(b), nil
}
func (m Message) Packed(n uint32, limit int) ([]uint64, error) {
	out := []uint64{}
	for _, f := range m.All(n) {
		if f.Kind == 0 {
			out = append(out, f.Uint)
		} else if f.Kind == 2 {
			b := f.Bytes
			for len(b) > 0 {
				v, k, e := varint(b)
				if e != nil {
					return nil, e
				}
				out = append(out, v)
				b = b[k:]
				if len(out) > limit {
					return nil, E("ResourceExhausted", "Packed field exceeds budget")
				}
			}
		} else {
			return nil, E("PluginProtocolError", "Wrong packed field kind")
		}
		if len(out) > limit {
			return nil, E("ResourceExhausted", "Packed field exceeds budget")
		}
	}
	return out, nil
}
func Uint(v uint64) []byte        { var b [10]byte; n := binary.PutUvarint(b[:], v); return b[:n] }
func V(n uint32, v uint64) []byte { return append(Uint(uint64(n)<<3), Uint(v)...) }
func B(n uint32, b []byte) []byte {
	out := append(Uint(uint64(n)<<3|2), Uint(uint64(len(b)))...)
	return append(out, b...)
}
func S(n uint32, s string) []byte { return B(n, []byte(s)) }
func Join(parts ...[]byte) []byte {
	out := []byte{}
	for _, p := range parts {
		out = append(out, p...)
	}
	return out
}

// Replace preserves every other wire field byte-for-byte, including unknown future fields.
func Replace(b []byte, n uint32, replacement []byte) ([]byte, error) {
	m, e := Parse(b)
	if e != nil {
		return nil, e
	}
	out := []byte{}
	found := false
	for _, f := range m {
		if f.Number == n {
			if found {
				return nil, E("PluginProtocolError", "Duplicate replacement field")
			}
			out = append(out, replacement...)
			found = true
		} else {
			out = append(out, f.Raw...)
		}
	}
	if !found {
		out = append(out, replacement...)
	}
	return out, nil
}

const Prefix = "type.googleapis.com/"

func Any(name string, payload []byte) []byte { return Join(S(1, Prefix+name), B(2, payload)) }
func Unpack(b []byte, expected string) ([]byte, error) {
	m, e := Parse(b)
	if e != nil {
		return nil, e
	}
	name, e := m.Text(1, 256)
	if e != nil {
		return nil, e
	}
	if name != Prefix+expected {
		return nil, E("PluginProtocolError", "Unexpected protobuf response type")
	}
	return m.Data(2)
}
func Request(token, client, name string, payload []byte) []byte {
	return Join(B(1, Join(S(1, token), S(2, client))), B(2, Any(name, payload)))
}

// Response never returns upstream error text. The caller retains the token privately.
func Response(b []byte, expected, token string) ([]byte, string, error) {
	m, e := Parse(b)
	if e != nil {
		return nil, "", e
	}
	h, e := m.Child(1)
	if e != nil {
		return nil, "", e
	}
	t, e := h.Text(1, 256)
	if e != nil {
		return nil, "", e
	}
	if t == "" {
		return nil, "", E("PluginProtocolError", "Missing KiCad instance identity")
	}
	if token != "" && t != token {
		return nil, "", E("StaleReference", "KiCad instance changed")
	}
	status, e := m.Child(2)
	if e != nil {
		return nil, "", e
	}
	code, e := status.Number(1)
	if e != nil {
		return nil, "", e
	}
	if code != 1 {
		return nil, "", Status(code)
	}
	a, e := m.Data(3)
	if e != nil {
		return nil, "", e
	}
	value, e := Unpack(a, expected)
	return value, t, e
}
