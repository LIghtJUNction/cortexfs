# Interaction ABI

`cortexfs-runtime-client` defines the provider-neutral interaction contract
shared by terminal, web, and IM Channel frontends. The current transport is a
local Unix stream socket. The protocol creates no `/ctx/interaction` namespace;
durable history remains under the existing session path.

## Implementation status

V2 frame types and validation are implemented in `cortexfs-runtime-client`.
The installed Agent socket service still executes the v1 one-request protocol.
Persistent v2 negotiation, attachments, replay, and session-scoped cancellation
below are a target contract, **not a deployed capability**. Do not send v2 frames
to production sockets or infer concurrent-session safety from these types.
Runtime activation requires durable per-session ownership, atomic sequence
allocation, authenticated handshakes, bounded writers, and recovery tests.
See [the harness delivery boundary](../harness.md).

Two compatible modes are specified:

```text
cortexfs.interaction/v2  persistent asynchronous stream; multiplexed attachments
cortexfs.interaction/v1  compatible one-request stream; send/resume/status/cancel
```

New persistent frontends MUST use `cortexfs.interaction/v2`. Version 1 remains
the one-request compatibility mode and retains its existing frame shapes and
semantics.

## Roles and independence

Every terminal, web, or IM frontend has the process role **Channel Master**.
Each logical Agent Session has one active **Agent Session Slave**. The role
names define ownership, not an election or privilege hierarchy: all attached
masters are peers, and no master becomes a leader for the session.

| Role | Owns | MUST NOT own |
| --- | --- | --- |
| Channel Master | frontend authentication, platform connection, interaction connection, attachment cursors, rendering, user input/cancel, command results, and platform side effects | durable session writes, run ordering, the Agent loop, or slave lifecycle |
| Agent Session Slave | session mailbox, authorization, single-writer ordering, durable append, run lifecycle, replay, request/command correlation, and bounded fan-out | terminal/web/IM presentation, platform codecs, or a reverse connection to a master |

Master and slave depend only on this neutral protocol. Neither side may import,
call back into, or identify the other side's implementation. A slave accepts
frames on its configured Unix socket and MUST NOT reverse-dial a master.

Independence is a protocol, state, and lifecycle boundary; it does not require
one operating-system process per session. The current per-Agent runtime may
host many isolated session slaves, each with its own mailbox, lock, ordering,
and state. A later release may place each slave in a separate process without
changing this ABI.

## Framing and negotiation

A v2 frame is one UTF-8 JSON value terminated by exactly one newline. Correlation
is separate from the extensible event body:

```json
{
  "abi": "cortexfs.interaction/v2",
  "correlation": {
    "connection_id": "master-connection-1",
    "attachment_id": null,
    "request_id": null,
    "run_id": null,
    "command_id": null
  },
  "session_seq": null,
  "event": {
    "side": "master",
    "kind": "hello",
    "capabilities": ["input", "observe", "status", "command_result", "invoke", "replay"],
    "mode": null,
    "session": null,
    "origin": null,
    "durable": false,
    "data": {
      "versions": ["cortexfs.interaction/v2"],
      "fallback_versions": ["cortexfs.interaction/v1"],
      "master_id": "discord.primary"
    }
  }
}
```

The first master frame MUST be `hello`. The master chooses a connection-local
`connection_id`; it is a correlation value, not authentication. The slave
authenticates the socket peer before replying, selects exactly one offered
persistent-stream version, and emits `welcome` with the same `connection_id`,
`side: "slave"`, the selected ABI, and finite connection limits in `event.data`.
All later frames on that connection MUST use the selected ABI.
`fallback_versions` is informational and is not selected in place. If no
offered persistent version is supported, the slave emits an `error` event when
it can do so safely and closes the connection. A master may then reconnect and
perform one v1 request; a live stream never changes version in place.

Unknown JSON fields MUST be ignored. A missing required field, unknown event
`kind`, illegal `side`/`kind` pairing, wrong selected `abi`, invalid UTF-8,
incomplete final line, or malformed JSON fails closed. A connection-level framing or handshake
failure closes the connection. An attachment-local validation failure rejects
only that attachment or request when the frame can be attributed safely.

The encoded frame, including its newline, MUST NOT exceed 256 KiB. Every
correlation identifier MUST be non-empty UTF-8 of at most 128 bytes. `welcome`
MUST advertise positive finite values for at least:

```text
max_attachments
max_pending_requests_per_attachment
max_pending_commands_per_attachment
max_unacked_events_per_attachment
max_outbound_bytes_per_attachment
max_outbound_bytes_per_connection
max_replay_events_per_batch
```

A host may advertise smaller deployment limits. The master MUST obey them; the
slave MUST reject excess work before allocating unbounded state.

## Multiplexed attachments and replay

One physical connection may carry multiple logical session attachments. An
`attachment_id` is chosen by the master, is unique within that connection, and
names exactly one authorized `(agent, scope, session)` tuple:

```json
{
  "abi": "cortexfs.interaction/v2",
  "correlation": {
    "connection_id": "master-connection-1",
    "attachment_id": "tab-7",
    "request_id": null,
    "run_id": null,
    "command_id": null
  },
  "session_seq": null,
  "event": {
    "side": "master",
    "kind": "attach",
    "capabilities": ["input", "observe", "status", "cancel", "command_result", "replay"],
    "mode": "interact",
    "session": "default",
    "origin": null,
    "durable": false,
    "data": {
      "agent": "executor",
      "scope": "private",
      "after_session_seq": 41
    }
  }
}
```

`mode` is `observe` or `interact`. Observe requires the `observe` capability and
permits only authorized durable projections, replay, status, acknowledgment,
and detach. Interact requires `input` and may additionally request observation,
cancellation, command-result, or invoke capabilities. The slave MUST reject an
unauthorized requested mode or capability rather than silently upgrade it.

The slave serializes attach against the session mailbox. On success it emits
`attached` with the authorized scope and mode, the replay start, and the
current durable head. It then emits durable events whose `session_seq` is
greater than the cursor, in ascending order, before live durable events for
that attachment.
This head-to-live transition MUST have no gap. Cursor `0` requests all retained
events. An omitted cursor starts at the current head and requests no historical
replay.

The master advances its local cursor only after it has processed the event. It
may send a cumulative `ack` containing the highest contiguous `session_seq`.
An acknowledgment releases delivery-buffer state; it does not delete or alter
session history. `detach` removes only that logical attachment. Closing a
multiplexed connection detaches all of its attachments but does not delete or
cancel a private/shared session.

## Frame families and correlation

V2 reserves these frame kinds and correlations:

| Family | Kinds | Correlation |
| --- | --- | --- |
| Connection | `hello`, `welcome` | `connection_id` |
| Attachment | `attach`, `attached`, `detach`, `ack`, `gap` | `connection_id`, `attachment_id` |
| Master operation | `input`, `status` | plus `request_id` |
| Active run | `cancel`, `accepted`, `started`, `event`, `done` | plus `request_id`, `run_id` |
| Runtime command | `command`, `command_result` | plus `request_id`, `run_id`, `command_id` |
| Error | `error` | the longest safely attributable correlation prefix |

`request_id` is master-issued and identifies an attempted session action.
`run_id` is slave-issued and identifies one accepted execution. `command_id` is
slave-issued and correlates exactly one `command_result`. A command that may
cause an external platform side effect carries a stable `effect_id` inside
`event.data`; the master deduplicates that effect independently of transport
retries. Durable delivery is deduplicated by `(session, session_seq)`.

`accepted`, `started`, and `done` are always durable and carry a positive
`session_seq`. `event`, slave `status`, and `error` may be durable or ephemeral;
`durable: true` and `session_seq` MUST either both be present or both be absent.
An `ack` is not a session fact: it carries the highest contiguous positive
`session_seq` while retaining `durable: false`.

An `input` may carry the existing provider-neutral `origin` and bounded `event`
objects. `origin` may describe transport, endpoint, external identity,
conversation, thread, and bounded metadata, but it MUST NOT introduce Telegram,
Discord, HTTP-provider, or model-provider wire types. Strings such as
`master_id` or an external identity are claims, not authority.

Commands use provider-neutral `approval`, `input`, `notify`, or `invoke`
values. A proactive command MUST name one currently authorized
`attachment_id`; there is no implicit “first”, “active”, or broadcast target.
A master MUST NOT execute a side effect twice for the same `effect_id`, and the
slave MUST accept an identical repeated `command_result` without applying it
twice. Reuse of `command_id` or `effect_id` with different content is a
protocol conflict.

## Ordering and audience

The Agent Session Slave is the only writer for its logical session. It accepts
mailbox items in one total order, appends a durable fact, assigns the next
strictly increasing `session_seq`, and only then publishes that fact. Multiple
masters do not race for a write lock and do not elect a primary. Concurrent
inputs are ordered by slave acceptance, not by client clocks or arrival times
observed at different masters.

For each accepted input, the slave records the originating attachment.
Run-local `delta` events and run-initiated approval or input commands MUST be
sent only to that origin attachment. Another master cannot approve a call by
copying its ids. If the origin disconnects while an approval or input command
is pending, the slave fails that command closed; it does not reroute it to an
observer. Loss of an origin-only delta does not affect durable history.

After append, durable message, tool, status, error, cancellation, and completion
facts may be delivered to every attached master authorized to observe that
session. Authorization is checked on attach and again before delivery when
policy or peer authority changes. An observer receives only the durable event
projection allowed by its authority; attachment does not grant control or
reveal secrets.

## Idempotency and delivery

Accepted requests and durable events use at-least-once delivery. A master may
retry an `input`, `cancel`, or other state-changing request with the same
`request_id`. The slave stores the canonical request claim in the session: an
identical retry replays the accepted/current/final result without executing the
action again, while reuse with a different payload, target session, scope, or
effective working directory fails with `request_conflict` (`EINVAL` in v1).

Durable events may be repeated before or after reconnect. Masters deduplicate by
`(session, session_seq)` and resume from the highest contiguous
processed cursor. The protocol does not promise exactly-once network delivery.
Live deltas are best-effort and are not replayed. A pending command may be
repeated on its same target attachment with the same `command_id` and
`effect_id`; the result and effect remain idempotent.

## Backpressure, failure, and reconnect

Every attachment has a bounded input mailbox, pending-command set, unacknowledged
event window, and outbound byte queue. Scheduling across attachments on one
connection MUST be fair enough that one replay or live run cannot allocate an
unbounded queue for the others. Implementations may coalesce or discard live
`delta` events first. They MUST NOT silently discard an accepted request or a
durable event.

When an input mailbox is full, the slave rejects new work with `busy` and does
not claim its `request_id`. When a durable outbound window reaches its limit,
the slave pauses that attachment. If the peer remains slow, it emits
`slow_consumer` when possible and detaches it; reconnect plus cursor replay is
the recovery path. If the connection-wide byte limit is reached, the slave
closes the connection rather than buffering without bound.

On master crash or disconnect, private/shared session execution and durable
append may continue. Origin-only live output is lost, and a pending origin-only
approval/input command fails closed. On slave crash, masters reconnect,
reauthenticate, reattach, and present their last processed cursor and stable
request ids. Before accepting new work, the recovered slave validates the
durable JSONL tail, reacquires the exclusive session lock, restores the next
`session_seq` and request claims, and replays durable facts. An accepted run
that has no durable terminal fact is not executed again from guesswork: after
receipt-bound orphan cleanup, recovery appends one interrupted/error terminal
fact, and an identical request retry replays that result. An incomplete or
unprovable tail is an `EIO`-class session error; it MUST NOT be guessed or
silently truncated.

## Peer authorization and split brain

Both endpoints MUST authenticate their Unix peer with `SO_PEERCRED`. A master
verifies that the slave UID is the expected session service owner (or the
specified root-owned service). The slave binds the kernel-provided UID/GID/PID
to the connection and authorizes every attachment against the requested Agent,
scope, session, policy, mount, and Linux ownership. A claimed master or external
identity never replaces this check.

A web or IM host is the Channel Master seen by the Unix socket. It MUST
authenticate its outer client or platform event and attenuate that identity in
the neutral origin; the slave does not treat the host's `SO_PEERCRED` as proof
of an arbitrary end user.

Exactly one slave may own a private/shared session for writes. Before attach or
recovery it MUST hold an exclusive, OS-released lock keyed by the canonical
session path, including its UID or shared-space identity, Agent, scope, and
session name. The lock is runtime state under `/run` or an equivalent
implementation-private location, not a new `/ctx` ABI entry. Failure to acquire
or retain it is `session_locked`; the contender MUST NOT append, issue commands,
or serve a second live stream. A lock owner that detects loss or conflicting
ownership stops admission and writing and closes its attachments. Takeover is
allowed only after the previous lock is released and the durable tail has been
validated. This is the fail-closed split-brain rule.

## Lower channel boundary

The interaction ABI is the Channel Master/Agent Session Slave layer:

```text
terminal / web / IM Channel Master
        ⇅  cortexfs.interaction/v2
Agent Session Slave
```

`cortexfs.channel.socket/v1` remains the lower platform-adapter boundary:

```text
platform adapter
        ⇅  cortexfs.channel.socket/v1
Channel Master <-> cortexfs.interaction/v2 <-> Agent Session Slave
```

Platform codecs translate native payloads into the existing provider-neutral
channel values and never enter Agent code. The two protocols are independently
versioned. This design adds no `/ctx` top-level object, watcher, polling loop,
hot reload, workflow/job/hook entry, reverse dial, or provider special case.

## Version 1 compatibility

A v1 connection carries exactly one existing request and its event stream. Its
version marker and envelope remain `cortexfs.interaction/v1`; `send`, `resume`,
`status`, `cancel`, and `command_result` retain their current meanings. A v1
client cannot multiplex attachments and does not perform the v2 handshake.
Existing request idempotency and session durability still apply.

The built-in one-shot HTTP POST remains a v1 surface. Persistent terminal,
WebSocket, and IM masters should negotiate v2. The Rust traits are compile-time
APIs, not a stable Rust binary ABI; external implementations use these JSONL
frames or another independently versioned process contract.
