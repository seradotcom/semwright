// SPDX-License-Identifier: GPL-3.0-or-later
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"runtime/debug"
	"semwright-kicad-native/internal/core"
	"semwright-kicad-native/internal/driverproto"
	"semwright-kicad-native/internal/wire"
)

func main() {
	runtime.GOMAXPROCS(2)
	debug.SetMemoryLimit(64 << 20)
	// Owner-only test tooling, never dispatched as an agent-facing capability.
	if len(os.Args) == 5 && os.Args[1] == "--catalog" {
		c := core.Catalog(os.Args[2] == "true", os.Args[3] == "true", os.Args[4] == "true")
		_ = json.NewEncoder(os.Stdout).Encode(c)
		return
	}
	path := core.DefaultConfig
	if len(os.Args) == 3 && os.Args[1] == "--config" {
		path = os.Args[2]
	} else if len(os.Args) != 1 {
		fmt.Fprintln(os.Stderr, "InvalidArgument: use no arguments, or --config OWNER_FILE for standalone testing")
		os.Exit(2)
	}
	c, err := core.ReadConfig(path)
	if err == nil {
		err = driverproto.Serve(os.Stdin, os.Stdout, c)
	}
	if err != nil {
		e := wire.AsError(err)
		fmt.Fprintln(os.Stderr, e.Code+": "+e.Message)
		os.Exit(1)
	}
}
