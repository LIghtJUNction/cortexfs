# Terminal broker ABI

Status: normative.

This document defines the authorization boundary between CortexFS terminal
clients and agent PTY supervisors. It is the only supported terminal transport
for newly started agents.

## 1. Endpoint and ownership

The broker is systemd socket activated at:

```text
/run/cortexfs/terminal/broker.sock
```

The socket and every parent directory are root-owned. The socket may be
world-connectable because possession of the path is not authorization. Clients
MUST authenticate the server with `SO_PEERCRED` and reject a non-root peer. The
broker MUST authenticate every client with `SO_PEERCRED` plus `/proc` cgroup and
process-generation metadata.

The projected agent entry remains:

```text
/ctx/agents/<agent>/<session>/terminal/main.sock
```

Its backing alias targets the broker endpoint. The old per-user listener and
the line protocol (`watch\n`, `attach\n`, or `emit\n`) are invalid.

## 2. Frame format

Control messages use a four-byte unsigned big-endian length followed by one
UTF-8 JSON object. A frame MUST contain 1 through 4096 payload bytes. Unknown
fields, unknown variants, malformed JSON, partial frames, and frames that do
not complete within one second MUST be rejected.

Every request carries the exact ABI string:

```text
cortexfs.terminal-broker/v1
```

Agent and session values use the object-name grammar. Client nonces are 144
bits of operating-system entropy encoded as 24 URL-safe Base64 characters.
The broker consumes each `(uid, nonce)` at most once in its bounded replay
window.

## 3. Supervisor lifecycle

A sandboxed `ctxterm` supervisor performs this sequence before spawning the
agent process:

1. send `register` with agent, session, and transient systemd unit;
2. receive a broker generation bound to peer PID and process start time;
3. open and configure the PTY without spawning the agent;
4. send `activate` with that generation;
5. receive `activated`, then spawn the agent.

The broker accepts registration only when the peer cgroup contains the exact
expected `cortexfs-agent-…-terminal.service` unit. Activation before agent
spawn prevents an agent process sharing the unit from racing its supervisor.
Only one live supervisor may own `(uid, agent, session)`.

Native terminals are disabled. They may return only after CortexFS assigns an
agent a Unix identity distinct from its operator.

## 4. Client grant transaction

An operator sends `connect` with agent, session, mode (`watch` or `attach`),
and a fresh nonce. The broker rejects clients inside any CortexFS agent
cgroup and requires the operator UID to equal the registered owner UID.

For an accepted request, the broker:

1. sends `offer` to the supervisor;
2. passes the already-authenticated client descriptor with `SCM_RIGHTS`;
3. waits for `prepared` carrying the same nonce;
4. sends `accepted` to the client;
5. sends `commit` to the supervisor and drops its descriptor copy.

A failed client acknowledgement produces `abort`; no stream becomes visible
to the supervisor before `commit`. The broker never relays PTY bytes.

## 5. Limits and failure behavior

The broker permits at most 64 concurrent handshake workers, 1024 registered
supervisors, and 1024 consumed nonces. A supervisor permits at most 16 terminal clients.
All protocol and descriptor transfers are bounded and use close-on-exec file
descriptors.

Broker restart closes every supervisor control connection. A supervisor MUST
then terminate its PTY child. Existing client streams terminate with that
supervisor; recovery starts a new terminal generation. There is no legacy
fallback, implicit reconnection, background polling, or hot reload.
