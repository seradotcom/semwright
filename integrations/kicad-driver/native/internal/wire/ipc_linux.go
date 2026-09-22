// SPDX-License-Identifier: GPL-3.0-or-later
// Direct implementation of SP IPC v0 + REQ/REP v0, without automatic retransmission.
package wire

import (
	"encoding/binary"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

type SocketIdentity struct {
	Device uint64
	Inode  uint64
	UID    uint32
}
type Peer struct {
	UID uint32 `json:"uid"`
	PID int32  `json:"pid"`
}

// SocketIdentityAt is deliberately scoped to an owner-provided path; no filesystem-wide discovery.
func SocketIdentityAt(path string) (SocketIdentity, error) {
	var out SocketIdentity
	if !filepath.IsAbs(path) || filepath.Clean(path) != path || len(path) > 107 || strings.ContainsAny(path, "\x00\r\n") {
		return out, E("InvalidArgument", "Socket path is not a canonical bounded absolute path")
	}
	cur := "/"
	for _, part := range strings.Split(strings.TrimPrefix(path, "/"), "/") {
		cur = filepath.Join(cur, part)
		st, e := os.Lstat(cur)
		if e != nil {
			return out, E("Unavailable", "Configured IPC endpoint is absent")
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return out, E("PermissionDenied", "Symlink in IPC path")
		}
		if cur != path {
			if !st.IsDir() {
				return out, E("PermissionDenied", "IPC ancestor is not a directory")
			}
			s := st.Sys().(*syscall.Stat_t)
			if s.Uid != uint32(os.Getuid()) && s.Uid != 0 {
				return out, E("PermissionDenied", "IPC ancestor has an unexpected owner")
			}
			// Root-owned sticky /tmp is permitted; the immediate socket parent must be private.
			if st.Mode().Perm()&0022 != 0 && !(s.Uid == 0 && st.Mode()&os.ModeSticky != 0) {
				return out, E("PermissionDenied", "Writable IPC ancestor")
			}
			continue
		}
		s := st.Sys().(*syscall.Stat_t)
		if st.Mode()&os.ModeSocket == 0 || s.Uid != uint32(os.Getuid()) {
			return out, E("PermissionDenied", "IPC endpoint is not an owned Unix socket")
		}
		out = SocketIdentity{uint64(s.Dev), s.Ino, s.Uid}
	}
	parent, e := os.Lstat(filepath.Dir(path))
	if e != nil || parent.Mode().Perm()&0077 != 0 {
		return out, E("PermissionDenied", "IPC socket directory must be owner-private")
	}
	return out, nil
}

type Channel struct {
	conn     *net.UnixConn
	path     string
	identity SocketIdentity
	Peer     Peer
	serial   uint32
	timeout  time.Duration
}

func Dial(path string, timeout time.Duration, expectedPID int32) (*Channel, error) {
	before, e := SocketIdentityAt(path)
	if e != nil {
		return nil, e
	}
	c, e := net.DialTimeout("unix", path, timeout)
	if e != nil {
		return nil, E("Unavailable", "Cannot connect to configured IPC endpoint")
	}
	u, ok := c.(*net.UnixConn)
	if !ok {
		c.Close()
		return nil, E("Internal", "Unexpected IPC transport")
	}
	fail := func(err error) (*Channel, error) { u.Close(); return nil, err }
	raw, e := u.SyscallConn()
	if e != nil {
		return fail(E("PermissionDenied", "Cannot inspect IPC peer"))
	}
	var cred *syscall.Ucred
	var credErr error
	if e = raw.Control(func(fd uintptr) {
		cred, credErr = syscall.GetsockoptUcred(int(fd), syscall.SOL_SOCKET, syscall.SO_PEERCRED)
	}); e != nil || credErr != nil || cred == nil {
		return fail(E("PermissionDenied", "Cannot authenticate IPC peer UID"))
	}
	if cred.Uid != uint32(os.Getuid()) || (expectedPID > 0 && cred.Pid != expectedPID) {
		return fail(E("PermissionDenied", "IPC peer identity does not match owner configuration"))
	}
	after, e := SocketIdentityAt(path)
	if e != nil || after != before {
		return fail(E("StaleReference", "IPC endpoint replaced during connection"))
	}
	if e = u.SetDeadline(time.Now().Add(timeout)); e != nil {
		return fail(E("Unavailable", "Cannot set IPC deadline"))
	}
	if _, e = u.Write([]byte{0, 83, 80, 0, 0, 48, 0, 0}); e != nil {
		return fail(E("Unavailable", "IPC negotiation failed"))
	}
	header := make([]byte, 8)
	if _, e = io.ReadFull(u, header); e != nil {
		return fail(E("Unavailable", "IPC negotiation timed out or disconnected"))
	}
	expected := []byte{0, 83, 80, 0, 0, 49, 0, 0}
	for i, v := range expected {
		if header[i] != v {
			return fail(E("PluginProtocolError", "Peer is not an SP REP v0 endpoint"))
		}
	}
	return &Channel{conn: u, path: path, identity: before, Peer: Peer{cred.Uid, cred.Pid}, timeout: timeout}, nil
}
func (c *Channel) Close() {
	if c.conn != nil {
		_ = c.conn.Close()
		c.conn = nil
	}
}
func (c *Channel) Exchange(payload []byte, mutation bool) ([]byte, error) {
	if c.conn == nil {
		return nil, E("Unavailable", "IPC connection is closed; explicit reconnect required")
	}
	current, e := SocketIdentityAt(c.path)
	if e != nil || current != c.identity {
		c.Close()
		return nil, E("StaleReference", "IPC endpoint changed before dispatch")
	}
	if len(payload) > MaxMessage || len(payload) == 0 {
		return nil, E("ResourceExhausted", "IPC request outside size budget")
	}
	if c.serial >= 0x7ffffffe {
		c.Close()
		return nil, E("ResourceExhausted", "IPC request identity exhausted")
	}
	c.serial++
	id := c.serial | 0x80000000
	failure := func(err error) ([]byte, error) {
		c.Close()
		if mutation {
			return nil, Uncertain(err)
		}
		return nil, err
	}
	if e = c.conn.SetDeadline(time.Now().Add(c.timeout)); e != nil {
		return failure(E("Unavailable", "Cannot set IPC deadline"))
	}
	msg := make([]byte, 13+len(payload))
	msg[0] = 1
	binary.BigEndian.PutUint64(msg[1:9], uint64(4+len(payload)))
	binary.BigEndian.PutUint32(msg[9:13], id)
	copy(msg[13:], payload)
	// A partial write already creates mutation uncertainty. Never resend or reconnect here.
	if _, e = c.conn.Write(msg); e != nil {
		return failure(E("Timeout", "IPC send failed; no automatic retry"))
	}
	header := make([]byte, 9)
	if _, e = io.ReadFull(c.conn, header); e != nil {
		return failure(E("Timeout", "IPC response timed out or connection was lost"))
	}
	size := binary.BigEndian.Uint64(header[1:])
	if header[0] != 1 || size < 4 || size > MaxMessage+4 {
		return failure(E("ResourceExhausted", "IPC response framing exceeds limits"))
	}
	body := make([]byte, int(size))
	if _, e = io.ReadFull(c.conn, body); e != nil {
		return failure(E("Timeout", "IPC response was truncated"))
	}
	if binary.BigEndian.Uint32(body[:4]) != id {
		return failure(E("PluginProtocolError", "IPC response request identity mismatch"))
	}
	return body[4:], nil
}
