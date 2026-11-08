# CortexFS project scope reset

**Status: product direction not selected.** The previous Agent-runtime and Linux /ctx filesystem designs are retired as active product plans. They remain in Git history and the documentation archive only.

This reset follows one rule: **do one thing, and do it well.** Do not preserve a feature merely because an earlier design specified it.

## What the repository contains today

The current default branch contains documentation, ABI descriptions, and reconstruction recipes. It does not contain a buildable product implementation. Therefore, this pass can retire product promises and simplify the documentation entry points; it cannot honestly claim to have removed runtime code that is not present.

## Retired from the active scope

Do not implement or restore these as part of the next product:

- Agent runtime, agent loop, model/provider routing, inference, and model registry.
- Conversation sessions, memory stores, summarization, compaction, and prompt orchestration.
- Frontends, chat UI, messaging channels, and multi-platform transport.
- Agent process launching, PTY management, sandboxing, namespaces, cgroups, network policy, and lifecycle supervision.
- A mandatory FUSE mount, /ctx ABI, or custom virtual filesystem.
- MCP aggregation/proxy infrastructure, custom SDKs, plugin/package managers, and private tool protocols.
- Web dashboards, installers, background daemons, databases, and workflow engines.

These items are not a backlog. They are explicit non-goals unless a future, approved problem statement proves that one is essential.

## What remains active

Only product discovery and validation are active until a new direction is approved:

1. Identify one recurring task for one clearly described user.
2. Observe the current workaround and test existing tools before writing code.
3. State the smallest useful output and the exact boundary of responsibility.
4. Reuse existing protocols, libraries, CLIs, and operating-system primitives.
5. Prefer a local, read-only or reversible prototype with no daemon, root access, database, or account signup.
6. Keep installation, upgrades, and removal obvious. A first successful use should take minutes, not require infrastructure setup.

Do not design a generic platform, abstraction framework, or extension ecosystem before a concrete task requires it.

## Decision gates before implementation

A candidate is eligible for a small prototype only when all of these are answered:

- Who has the problem, and how often does it occur?
- What do they use today, and why is that insufficient?
- What existing projects already solve most of it?
- What is the one operation the first version performs?
- Can a new user try it in under five minutes using tools they likely already have?
- Can the first prototype be built in at most three focused development days by reusing existing components?

After the prototype, require real task trials from at least three people in the target group and evidence that at least two use it again on a later task. These are project stop/go thresholds, not market statistics. If existing tools are sufficient, contribute upstream or stop. Do not turn a failed candidate into a broader platform.

## Documentation and compatibility policy

- The docs/ and recipes/ directories describe retired designs and historical snapshots. They are references, not active implementation requirements.
- Existing ABI documents do not create a requirement to preserve or reimplement the old product.
- Do not update old contracts to anticipate an unknown product. Write a new, short specification only after a problem passes the decision gates.
- Keep the old documents available for provenance; do not expose them as the current product roadmap.
- Keep the repository license and useful engineering history.

## Claim-evidence check

| Claim | Evidence | Status |
| --- | --- | --- |
| The current checkout has no buildable product source | Current main README and repository instructions explicitly state that product source, installers, tests, and website implementation were removed | Supported by repository documentation |
| The previous design is too broad to carry forward unchanged | It includes runtime, model, session, channel, process, FUSE, adapter, installation, and web documentation | Supported by the retained design documents |
| A new product direction will have users | No user trials or adoption evidence yet | Not established |
| A small, dependency-light prototype is feasible | Must be assessed for the specific problem after selection | Needs evidence |

## Review checklist

- Contribution: Does the candidate solve one recurring problem not already handled adequately?
- Clarity: Can its purpose be explained in one sentence without saying "platform" or "framework"?
- Evidence: Are user pain and competitor limitations observed rather than assumed?
- Evaluation: Is there a repeat-use test and an explicit stop condition?
- Method: Does the design reuse existing components and avoid unnecessary infrastructure?
