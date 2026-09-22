// SPDX-License-Identifier: GPL-3.0-or-later
package core

import (
	"fmt"
	"regexp"
	"semwright-kicad-native/internal/wire"
	"strconv"
)

type Version struct {
	Major     uint64 `json:"major"`
	Minor     uint64 `json:"minor"`
	Patch     uint64 `json:"patch"`
	Full      string `json:"full"`
	Supported bool   `json:"supported"`
}

var versionPattern = regexp.MustCompile(`^([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})([-+][A-Za-z0-9._+-]{1,96})?$`)

func ParseVersion(s string) (Version, error) {
	m := versionPattern.FindStringSubmatch(s)
	if m == nil {
		return Version{}, wire.E("Unsupported", "Unrecognized KiCad version identifier")
	}
	nums := [3]uint64{}
	for i := range nums {
		v, e := strconv.ParseUint(m[i+1], 10, 16)
		if e != nil {
			return Version{}, wire.E("Unsupported", "Invalid version component")
		}
		nums[i] = v
	}
	v := Version{Major: nums[0], Minor: nums[1], Patch: nums[2], Full: s}
	v.Supported = (v.Major == 9 || v.Major == 10) && v.Minor < 90 && m[4] == ""
	return v, nil
}
func decodeVersion(b []byte) (Version, error) {
	r := read(b)
	v := r.child(1)
	major, minor, patch := v.u(1), v.u(2), v.u(3)
	full := v.s(4, 128)
	if r.err != nil {
		return Version{}, r.err
	}
	if v.err != nil {
		return Version{}, v.err
	}
	if major == 0 || major > 999 || minor > 999 || patch > 999 {
		return Version{}, wire.E("PluginProtocolError", "Invalid KiCad version response")
	}
	// Full strings can contain distro annotations. Advertise only a strict, stable numeric full string.
	parsed, e := ParseVersion(full)
	if e != nil {
		parsed = Version{Full: full}
	}
	parsed.Major = major
	parsed.Minor = minor
	parsed.Patch = patch
	canonical := fmt.Sprintf("%d.%d.%d", major, minor, patch)
	parsed.Supported = parsed.Supported && full == canonical
	return parsed, nil
}
