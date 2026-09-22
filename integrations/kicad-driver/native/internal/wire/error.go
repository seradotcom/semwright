// SPDX-License-Identifier: GPL-3.0-or-later
package wire

// Error carries only driver-owned text. Upstream text, paths and tokens never enter it.
type Error struct {
	Code         string   `json:"code"`
	Message      string   `json:"message"`
	Candidates   []string `json:"candidates,omitempty"`
	OutcomeKnown bool     `json:"outcome_known"`
}

func (e *Error) Error() string      { return e.Code + ": " + e.Message }
func E(code, message string) *Error { return &Error{Code: code, Message: message, OutcomeKnown: true} }
func Uncertain(err error) *Error {
	e := AsError(err)
	copy := *e
	copy.OutcomeKnown = false
	return &copy
}
func AsError(err error) *Error {
	if e, ok := err.(*Error); ok {
		return e
	}
	return E("BackendFailed", "Native core failed; upstream details withheld")
}
func Status(code uint64) *Error {
	switch code {
	case 2:
		return E("Timeout", "KiCad reported a timeout")
	case 3:
		return E("InvalidArgument", "KiCad rejected the request")
	case 4:
		return E("Unavailable", "KiCad editor is not ready")
	case 5, 8:
		return E("Unsupported", "Operation is not implemented by this KiCad instance")
	case 6:
		return E("StaleReference", "KiCad instance token changed")
	case 7:
		return E("Unavailable", "KiCad editor is busy")
	default:
		return E("BackendFailed", "Unrecognized KiCad status")
	}
}
