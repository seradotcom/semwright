// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"bytes"
	"encoding/json"
	"io"
	"math"
	"regexp"
	"semwright-kicad-native/internal/wire"
	"unicode/utf8"
)

// StrictJSON rejects duplicate keys, non-UTF8 input, excessive nesting and trailing values.
func StrictJSON(data []byte, into any) error {
	if len(data) == 0 || len(data) > wire.MaxMessage || !utf8.Valid(data) {
		return wire.E("InvalidArgument", "JSON input is empty, invalid UTF-8 or too large")
	}
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	nodes := 0
	var walk func(int) error
	walk = func(depth int) error {
		nodes++
		if depth > 32 || nodes > 16384 {
			return wire.E("ResourceExhausted", "JSON structural budget exceeded")
		}
		t, e := dec.Token()
		if e != nil {
			return wire.E("InvalidArgument", "Malformed JSON")
		}
		if d, ok := t.(json.Delim); ok {
			switch d {
			case '{':
				seen := map[string]bool{}
				for dec.More() {
					k, e := dec.Token()
					if e != nil {
						return wire.E("InvalidArgument", "Malformed object")
					}
					key, ok := k.(string)
					if !ok || seen[key] {
						return wire.E("InvalidArgument", "Duplicate JSON object key")
					}
					seen[key] = true
					if e = walk(depth + 1); e != nil {
						return e
					}
				}
			case '[':
				for dec.More() {
					if e = walk(depth + 1); e != nil {
						return e
					}
				}
			default:
				return wire.E("InvalidArgument", "Malformed delimiter")
			}
			if _, e = dec.Token(); e != nil {
				return wire.E("InvalidArgument", "Unclosed JSON collection")
			}
		}
		return nil
	}
	if e := walk(0); e != nil {
		return e
	}
	if _, e := dec.Token(); e != io.EOF {
		return wire.E("InvalidArgument", "Trailing JSON data")
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.UseNumber()
	d.DisallowUnknownFields()
	if e := d.Decode(into); e != nil {
		return wire.E("InvalidArgument", "JSON does not match the strict schema")
	}
	return nil
}
func object(props map[string]any, required ...string) map[string]any {
	if props == nil {
		props = map[string]any{}
	}
	if required == nil {
		required = []string{}
	}
	return map[string]any{"type": "object", "properties": props, "required": required, "additionalProperties": false}
}
func text(max int) map[string]any { return map[string]any{"type": "string", "maxLength": max} }
func integer(min, max int64) map[string]any {
	return map[string]any{"type": "integer", "minimum": min, "maximum": max}
}
func array(item any, max int) map[string]any {
	return map[string]any{"type": "array", "items": item, "maxItems": max}
}
func boolean() map[string]any       { return map[string]any{"type": "boolean"} }
func constant(v any) map[string]any { return map[string]any{"const": v} }

var refPattern = regexp.MustCompile(`^kc:[a-f0-9]{32}:[a-f0-9]{32}$`)
var digestPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
var slugPattern = regexp.MustCompile(`^[a-z][a-z0-9-]{0,38}[a-z0-9]$|^[a-z]$`)
var uuidPattern = regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$`)

func refSchema() map[string]any {
	return map[string]any{"type": "string", "pattern": refPattern.String(), "maxLength": 68}
}
func digestSchema() map[string]any {
	return map[string]any{"type": "string", "pattern": digestPattern.String(), "maxLength": 64}
}

// The curated input language is intentionally small. Broker JSON-Schema validation remains authoritative.
func validateArgs(schema map[string]any, args map[string]any) error {
	if args == nil {
		return wire.E("InvalidArgument", "Command arguments must be an object")
	}
	props := schema["properties"].(map[string]any)
	for _, k := range schema["required"].([]string) {
		if _, ok := args[k]; !ok {
			return wire.E("InvalidArgument", "Required command argument missing")
		}
	}
	for k, v := range args {
		p, ok := props[k].(map[string]any)
		if !ok {
			return wire.E("InvalidArgument", "Unexpected command argument")
		}
		switch p["type"] {
		case "string":
			s, ok := v.(string)
			if !ok {
				return wire.E("InvalidArgument", "String argument required")
			}
			if max, ok := p["maxLength"].(int); ok && len([]rune(s)) > max {
				return wire.E("InvalidArgument", "String argument exceeds bounds")
			}
			if pattern, ok := p["pattern"].(string); ok && !regexp.MustCompile(pattern).MatchString(s) {
				return wire.E("InvalidArgument", "Argument format is invalid")
			}
		case "integer":
			n, ok := v.(json.Number)
			if !ok {
				return wire.E("InvalidArgument", "Integer argument required")
			}
			value, e := n.Int64()
			if e != nil {
				return wire.E("InvalidArgument", "Integer argument outside representation")
			}
			if value < p["minimum"].(int64) || value > p["maximum"].(int64) {
				return wire.E("InvalidArgument", "Integer argument outside bounds")
			}
		case "number":
			n, ok := v.(json.Number)
			if !ok {
				return wire.E("InvalidArgument", "Number required")
			}
			f, e := n.Float64()
			if e != nil || math.IsNaN(f) || math.IsInf(f, 0) {
				return wire.E("InvalidArgument", "Invalid finite number")
			}
		default:
			return wire.E("Internal", "Unimplemented curated input schema")
		}
		if enums, ok := p["enum"].([]string); ok {
			found := false
			for _, s := range enums {
				if v == s {
					found = true
				}
			}
			if !found {
				return wire.E("InvalidArgument", "Unknown argument choice")
			}
		}
	}
	return nil
}
func asInt(args map[string]any, k string, def int64) int64 {
	if n, ok := args[k].(json.Number); ok {
		v, e := n.Int64()
		if e == nil {
			return v
		}
	}
	return def
}
