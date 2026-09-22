// SPDX-License-Identifier: GPL-3.0-or-later
package main

/*
#include <stdint.h>
#include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"runtime"
	"runtime/debug"
	"semwright-kicad-native/internal/core"
	"semwright-kicad-native/internal/wire"
	"sync"
	"sync/atomic"
	"unsafe"
)

var engines sync.Map
var next atomic.Uint64

func init() { runtime.GOMAXPROCS(2); debug.SetMemoryLimit(64 << 20) }
func response(v any, err error) *C.char {
	var b []byte
	if err != nil {
		b, _ = json.Marshal(map[string]any{"error": wire.AsError(err)})
	} else {
		b, err = json.Marshal(map[string]any{"value": v})
		if err != nil || len(b) > wire.MaxMessage {
			b = []byte(`{"error":{"code":"ResourceExhausted","message":"C ABI output budget exceeded","outcome_known":true}}`)
		}
	}
	return C.CString(string(b))
}
func protected(out **C.char) {
	if recover() != nil {
		*out = response(nil, wire.Uncertain(wire.E("Internal", "Native core failed; discard this driver process")))
	}
}

//export KiOpen
func KiOpen(path *C.char, n C.int) (out *C.char) {
	defer protected(&out)
	if path == nil || n <= 0 || n > 4096 {
		return response(nil, wire.E("InvalidArgument", "Invalid owner configuration path"))
	}
	c, err := core.ReadConfig(C.GoStringN(path, n))
	if err != nil {
		return response(nil, err)
	}
	e, err := core.New(c)
	if err != nil {
		return response(nil, err)
	}
	id := next.Add(1)
	if id == 0 {
		e.Close()
		return response(nil, wire.E("ResourceExhausted", "Handle space exhausted"))
	}
	engines.Store(id, e)
	return response(map[string]any{"handle": id}, nil)
}

//export KiCall
func KiCall(handle C.uint64_t, p *C.char, n C.int) (out *C.char) {
	defer protected(&out)
	if p == nil || n <= 0 || n > wire.MaxMessage {
		return response(nil, wire.E("InvalidArgument", "Invalid C ABI input bounds"))
	}
	value, ok := engines.Load(uint64(handle))
	if !ok {
		return response(nil, wire.E("StaleReference", "Unknown native core handle"))
	}
	e := value.(*core.Engine)
	var r struct {
		Operation string         `json:"operation"`
		Command   string         `json:"command,omitempty"`
		Digest    string         `json:"descriptor_sha256,omitempty"`
		Args      map[string]any `json:"args,omitempty"`
	}
	if err := core.StrictJSON(C.GoBytes(unsafe.Pointer(p), n), &r); err != nil {
		return response(nil, err)
	}
	switch r.Operation {
	case "catalog":
		v, err := e.Capabilities()
		return response(v, err)
	case "health":
		v, err := e.Health()
		return response(v, err)
	case "execute":
		v, err := e.Execute(r.Command, r.Digest, r.Args)
		return response(v, err)
	default:
		return response(nil, wire.E("Unsupported", "Unknown internal C ABI operation"))
	}
}

//export KiClose
func KiClose(handle C.uint64_t) {
	if e, ok := engines.LoadAndDelete(uint64(handle)); ok {
		e.(*core.Engine).Close()
	}
}

//export KiFreeString
func KiFreeString(p *C.char) { C.free(unsafe.Pointer(p)) }
func main()                  {}
