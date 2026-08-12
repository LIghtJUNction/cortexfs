# Terminal Resource ABI

This document defines the first durable terminal-resource slice. A terminal is
not a one-shot shell result: it is a resource owned by a durable session and
realized by a replaceable process, PTY, and live transport.

## Scope and root boundary

The current root ABI remains frozen. The resource is therefore projected below
the existing Agent session:

~~~text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/<terminal-id>/
  meta.json
  state
  status
  owner
  cwd
  events.jsonl
~~~

This path is a session-local resource, not a new top-level root class. A future
/ctx/terminal projection needs a separately versioned root-ABI decision and
must not silently create a second lifecycle or submission namespace.

The implementation currently creates one Agent-backed terminal for an Agent
start. A detached command supervisor, generic command creation, and multiple
independent process objects are reserved for a later revision.

## Identity and layers

The stable id for the compatibility terminal is:

~~~text
terminal-<agent>-<session>
~~~

The id is derived from the Agent and session names and is not the socket path.
The layers are:

~~~text
resource     terminal/<terminal-id>/ ordinary files
instance     one supervised launch receipt and invocation
process      the ctxterm child command
PTY          portable-pty master/slave pair
attachment   zero or more watch or attach clients
~~~

The session owns the resource directory. The supervisor owns live process
truth. A socket is only a locator for an active PTY and may disappear without
deleting metadata or history.

## Files

meta.json is the complete inspectable record:

~~~json
{
  "id": "terminal-coder-default",
  "agent": "coder",
  "session": "default",
  "owner": "1000",
  "cwd": "/workspace",
  "command": ["/ctx/bin/tsh"],
  "state": "running",
  "socket": "/run/user/1000/cortexfs/terminal/coder/default/main.sock",
  "created_at": 1735689600
}
~~~

state is the canonical short text projection; status is a compatibility alias.
Implementations may use created, running, exited, or error. owner and cwd are
text projections of the record. All
metadata writes use the normal temporary-file plus same-directory atomic rename
rule.

events.jsonl is append-only, one JSON object per line. PTY output is byte
oriented and is encoded as standard base64:

~~~json
{"seq":1,"ts":1735689601,"type":"pty.output","data_b64":"Y2FyZ28gYnVpbGQK"}
{"seq":2,"ts":1735689602,"type":"process.exit","exit_code":0}
~~~

seq is monotonic within one resource. type is currently pty.output or
process.exit. A PTY combines stdout and stderr by design; consumers must not
infer a separate stream from the event type. A later ABI may add explicit
stream labels without changing the meaning of existing events.

## Commands

The human CLI is a thin client over this resource:

~~~text
ctx terminal create AGENT [--session SESSION] [--cwd PATH]
ctx terminal list
ctx terminal status TERMINAL
ctx terminal watch TERMINAL
ctx terminal attach TERMINAL
~~~

create prepares the durable session resource and starts the existing supervised
Agent terminal. list and status are read-only. watch receives PTY bytes without
writing input. attach receives PTY bytes and forwards input, matching the
existing Agent terminal attach behavior. Neither list nor status starts a
runtime or changes process state.

## Capabilities

Terminal authority is capability-shaped even while the current CLI is
Agent-backed:

~~~text
terminal.create
terminal.read
terminal.write
terminal.resize
terminal.signal
terminal.stop
terminal.kill
~~~

The policy layer must grant each operation independently. A caller that can
read or write a PTY does not automatically gain signal or kill authority.
Current watch and attach routes reuse the existing Agent socket policy; the
capability names are reserved for the detached terminal supervisor revision.

## Attach and lifetime

Human and Agent attachments are peers of the terminal resource:

~~~text
terminal resource
  +-- Agent attachment
  +-- human watch
  +-- human attach
  +-- another authorized attachment
~~~

The resource outlives a particular attachment and retains events after the PTY
exits. Agent teardown may stop the current compatibility process, but it does
not remove the session resource or its event history. A future detached
supervisor must make this lifetime independence the default.
