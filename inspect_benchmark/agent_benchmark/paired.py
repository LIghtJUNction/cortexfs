from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import math
import random
import statistics
from collections import defaultdict
from collections.abc import Mapping, Sequence
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .agents import record_process_cleanup_complete, sanitize
from .pi import PiJsonlRunner
from .pi_system import (
    ApiPriceTable,
    _add_price_arguments,
    _binary_identity,
    _pi_observation,
    _price_table,
    _provider_family,
)
from .system import (
    _aggregate,
    _benchmark_source_identity,
    _comparison_fingerprint,
    _ctx_observation,
    _dataset,
    _dataset_identity,
    _jsonl_content,
    _percentile,
    _preflight,
    _publish_output,
    _require_cleanup,
    _runtime_contracts_complete,
    _runtime_has_no_visible_tools,
    _runtime_identity,
    _runtime_snapshots_stable,
    _sanitized_dict,
    _unit_runtime_snapshot,
    _validate_model_controls,
)

SCHEDULE = ("A", "B", "B", "A")
BOOTSTRAP_SEED = 0xC07E5F5
BOOTSTRAP_RESAMPLES = 2000


def _runtime_for(mode: str, arm: str) -> str:
    if mode == "compare":
        return "ctx" if arm == "A" else "pi"
    if mode == "aa-ctx":
        return "ctx"
    if mode == "aa-pi":
        return "pi"
    raise ValueError(f"unknown paired mode: {mode}")


def _pair_records(samples: Sequence[Mapping[str, object]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[object, object, object], dict[str, Mapping[str, object]]] = (
        defaultdict(dict)
    )
    for record in samples:
        key = (record.get("sample_id"), record.get("epoch"), record.get("pair"))
        arm = record.get("arm")
        if arm not in {"A", "B"} or arm in grouped[key]:
            raise ValueError(f"invalid or duplicate paired arm for {key}")
        grouped[key][str(arm)] = record
    pairs: list[dict[str, Any]] = []
    for key, arms in grouped.items():
        if set(arms) != {"A", "B"}:
            raise ValueError(f"incomplete paired observation for {key}")
        left, right = arms["A"], arms["B"]
        left_success = bool(left.get("runtime_success"))
        right_success = bool(right.get("runtime_success"))
        left_exact = left_success and bool(left.get("exact_match"))
        right_exact = right_success and bool(right.get("exact_match"))
        both_success = left_success and right_success
        usage_eligible = bool(left.get("usage_available")) and bool(
            right.get("usage_available")
        )
        pair: dict[str, Any] = {
            "sample_id": key[0],
            "epoch": key[1],
            "pair": key[2],
            "order": left.get("order"),
            "a_runtime": left.get("runtime"),
            "b_runtime": right.get("runtime"),
            "a_success": left_success,
            "b_success": right_success,
            "quality_delta": (1.0 if left_exact else 0.0)
            - (1.0 if right_exact else 0.0),
            "success_delta": (1.0 if left_success else 0.0)
            - (1.0 if right_success else 0.0),
            "latency_improvement": None,
            "token_improvement": None,
            "api_cost_improvement": None,
            "latency_eligible": both_success,
            "usage_eligible": usage_eligible,
            "api_cost_eligible": False,
        }
        if both_success:
            left_latency = _positive(left.get("latency_seconds"))
            right_latency = _positive(right.get("latency_seconds"))
            if left_latency is not None and right_latency is not None:
                pair["latency_improvement"] = 1 - left_latency / right_latency
        if usage_eligible:
            left_tokens = _positive(left.get("comparable_tokens"))
            right_tokens = _positive(right.get("comparable_tokens"))
            if left_tokens is not None and right_tokens is not None:
                pair["token_improvement"] = 1 - left_tokens / right_tokens
            left_cost = _positive(left.get("api_equivalent_cost_usd"))
            right_cost = _positive(right.get("api_equivalent_cost_usd"))
            if left_cost is not None and right_cost is not None:
                pair["api_cost_eligible"] = True
                pair["api_cost_improvement"] = 1 - left_cost / right_cost
        pairs.append(pair)
    return sorted(
        pairs,
        key=lambda pair: (
            str(pair["sample_id"]),
            _index(pair["epoch"], "epoch"),
            _index(pair["pair"], "pair"),
        ),
    )


def _index(value: object, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        raise ValueError(f"invalid paired {field}")
    return value


def _positive(value: object) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
        return None
    return value * 1.0


def _cluster_bootstrap(
    entries: Sequence[tuple[str, float]],
) -> dict[str, float | int | None]:
    if not entries:
        return {
            "observations": 0,
            "independent_samples": 0,
            "mean": None,
            "lower_95": None,
            "upper_95": None,
        }
    clusters: dict[str, list[float]] = defaultdict(list)
    for sample_id, value in entries:
        clusters[sample_id].append(value)
    identifiers = sorted(clusters)
    observed = [value for values in clusters.values() for value in values]
    generator = random.Random(BOOTSTRAP_SEED)
    means: list[float] = []
    for _resample in range(BOOTSTRAP_RESAMPLES):
        selected = [generator.choice(identifiers) for _item in identifiers]
        values = [value for identifier in selected for value in clusters[identifier]]
        means.append(statistics.fmean(values))
    return {
        "observations": len(observed),
        "independent_samples": len(identifiers),
        "mean": statistics.fmean(observed),
        "lower_95": _percentile(means, 0.025),
        "upper_95": _percentile(means, 0.975),
    }


def _paired_metrics(pairs: Sequence[Mapping[str, object]], mode: str) -> dict[str, Any]:
    fields = (
        "quality_delta",
        "success_delta",
        "latency_improvement",
        "token_improvement",
        "api_cost_improvement",
    )
    metrics: dict[str, Any] = {}
    for field in fields:
        entries = [
            (str(pair.get("sample_id")), value * 1.0)
            for pair in pairs
            if isinstance((value := pair.get(field)), (int, float))
            and not isinstance(value, bool)
        ]
        metrics[field] = _cluster_bootstrap(entries)
    noise: dict[str, float | None] = {}
    if mode.startswith("aa-"):
        # Keep every AB/BA pair for the conservative noise envelope; averaging a
        # task cluster first can cancel the order effect this gate must expose.
        for field in (
            "latency_improvement",
            "token_improvement",
            "api_cost_improvement",
        ):
            log_ratios: list[float] = []
            for pair in pairs:
                value = pair.get(field)
                if isinstance(value, bool) or not isinstance(value, (int, float)):
                    continue
                ratio = 1 - value
                if ratio <= 0:
                    raise ValueError(f"invalid A/A ratio for {field}")
                log_ratios.append(abs(math.log(ratio)))
            percentile = _percentile(log_ratios, 0.95)
            noise[field] = math.exp(percentile) - 1 if percentile is not None else None
        quality = [
            abs(value * 1.0)
            for pair in pairs
            if isinstance((value := pair.get("quality_delta")), (int, float))
            and not isinstance(value, bool)
        ]
        noise["quality_delta"] = _percentile(quality, 0.95)
    independent_samples = len({str(pair.get("sample_id")) for pair in pairs})
    metrics["coverage"] = {
        "expected_pairs": len(pairs),
        "both_runtime_success": sum(
            bool(pair.get("a_success") and pair.get("b_success")) for pair in pairs
        ),
        "usage_eligible_pairs": sum(bool(pair.get("usage_eligible")) for pair in pairs),
        "token_metric_pairs": sum(
            pair.get("token_improvement") is not None for pair in pairs
        ),
        "api_cost_eligible_pairs": sum(
            bool(pair.get("api_cost_eligible")) for pair in pairs
        ),
        "independent_task_samples": independent_samples,
    }
    metrics["aa_noise_p95"] = noise or None
    metrics["p95_is_descriptive"] = any(
        int(metrics[field]["independent_samples"]) < 100 for field in fields
    )
    return metrics


async def _run_warmups(
    *,
    mode: str,
    count: int,
    ctx_path: str,
    agent_name: str,
    pi_runner: PiJsonlRunner | None,
    sample: Mapping[str, object],
    timeout: float,
    retain_frames: bool,
    price_table: ApiPriceTable | None,
) -> list[dict[str, Any]]:
    runtimes = list(dict.fromkeys(_runtime_for(mode, arm) for arm in ("A", "B")))
    records: list[dict[str, Any]] = []
    for runtime in runtimes:
        for index in range(count):
            if runtime == "ctx":
                record = dict(
                    await _ctx_observation(
                        ctx_path=ctx_path,
                        agent_name=agent_name,
                        sample=sample,
                        sample_index=0,
                        epoch=-1,
                        timeout=timeout,
                        retain_frames=retain_frames,
                        measure_memory=False,
                    )
                )
                record["runtime"] = "ctx"
            else:
                if pi_runner is None:
                    raise ValueError("Pi runner is required by this warm-up")
                record = dict(
                    await _pi_observation(
                        runner=pi_runner,
                        sample=sample,
                        sample_index=0,
                        epoch=-1,
                        price_table=price_table,
                    )
                )
                record["runtime"] = "pi"
            record.update({"warmup": True, "warmup_index": index + 1, "mode": mode})
            records.append(record)
    return records


async def _run_schedule(
    *,
    mode: str,
    ctx_path: str,
    agent_name: str,
    pi_runner: PiJsonlRunner | None,
    dataset: Sequence[dict[str, Any]],
    repeat: int,
    timeout: float,
    retain_frames: bool,
    measure_memory: bool,
    price_table: ApiPriceTable | None,
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for epoch in range(repeat):
        for sample_index, sample in enumerate(dataset):
            for position, arm in enumerate(SCHEDULE):
                runtime = _runtime_for(mode, arm)
                if runtime == "ctx":
                    record = dict(
                        await _ctx_observation(
                            ctx_path=ctx_path,
                            agent_name=agent_name,
                            sample=sample,
                            sample_index=sample_index,
                            epoch=epoch,
                            timeout=timeout,
                            retain_frames=retain_frames,
                            measure_memory=measure_memory,
                        )
                    )
                    record["runtime"] = "ctx"
                else:
                    if pi_runner is None:
                        raise ValueError("Pi runner is required by this paired mode")
                    record = dict(
                        await _pi_observation(
                            runner=pi_runner,
                            sample=sample,
                            sample_index=sample_index,
                            epoch=epoch,
                            price_table=price_table,
                        )
                    )
                record.update(
                    {
                        "arm": arm,
                        "position": position + 1,
                        "pair": 1 if position < 2 else 2,
                        "order": "AB" if position < 2 else "BA",
                        "mode": mode,
                    }
                )
                records.append(record)
    return records


def _object(value: object, field: str) -> Mapping[str, object]:
    if not isinstance(value, dict):
        raise ValueError(f"paired report field must be an object: {field}")
    return value


def _process_cleanup_complete(records: Sequence[Mapping[str, object]]) -> bool:
    return bool(records) and all(
        record_process_cleanup_complete(record) for record in records
    )


def _nonnegative_integer_field(record: Mapping[str, object], field: str) -> bool:
    value = record.get(field)
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _usage_evidence_complete(records: Sequence[Mapping[str, object]]) -> bool:
    return bool(records) and all(
        bool(record.get("usage_available"))
        and record.get("usage_semantics") == "visible-input-output-v1"
        and all(
            _nonnegative_integer_field(record, field)
            for field in ("input_tokens", "output_tokens", "comparable_tokens")
        )
        for record in records
    )


def _memory_evidence_complete(records: Sequence[Mapping[str, object]]) -> bool:
    return bool(records) and all(
        bool(record.get("memory_complete"))
        and _nonnegative_integer_field(record, "peak_memory_bytes")
        for record in records
    )


def _evidence_status(gates: Mapping[str, object]) -> str:
    failed = sorted(
        name for name, value in gates.items() if name != "all_pass" and not bool(value)
    )
    return "complete" if not failed else "inconclusive-pending-" + "-and-".join(failed)


def _runtime_process_identity_complete(
    records: Sequence[Mapping[str, object]],
) -> bool:
    ctx_records = [record for record in records if record.get("runtime") == "ctx"]
    return not ctx_records or all(
        isinstance((evidence := record.get("runtime_process_evidence")), dict)
        and bool(evidence.get("complete"))
        for record in ctx_records
    )


def _warmups_valid(
    records: Sequence[Mapping[str, object]], mode: str, count: int
) -> bool:
    runtime_count = len({_runtime_for(mode, arm) for arm in ("A", "B")})
    return (
        count > 0
        and len(records) == count * runtime_count
        and all(
            bool(record.get("runtime_success"))
            and bool(record.get("raw_frames_complete"))
            and (record.get("runtime") != "ctx" or bool(record.get("cleanup_success")))
            for record in records
        )
        and _usage_evidence_complete(records)
        and _process_cleanup_complete(records)
        and _runtime_process_identity_complete(records)
    )


def _report(summary: Mapping[str, object]) -> str:
    paired = _object(summary["paired"], "paired")
    experiment = _object(summary["experiment"], "experiment")
    gates = _object(summary["evidence_gates"], "evidence_gates")
    lines = [
        "# Paired CortexFS / Pi benchmark",
        "",
        f"Mode: `{summary['mode']}`",
        f"Schedule: `{experiment['schedule']}`",
        f"Workload fingerprint: `{summary['fingerprint']}`",
        f"Evidence status: **{summary['evidence_status']}**",
        "",
        "| Metric | Observations | Independent tasks | Mean | Bootstrap 95% lower | Bootstrap 95% upper |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for field in (
        "quality_delta",
        "success_delta",
        "latency_improvement",
        "token_improvement",
        "api_cost_improvement",
    ):
        metric = _object(paired[field], field)
        lines.append(
            f"| {field} | {metric['observations']} | {metric['independent_samples']} | {metric['mean']} | {metric['lower_95']} | {metric['upper_95']} |"
        )
    lines.extend(
        [
            "",
            f"A/A noise p95: `{json.dumps(paired.get('aa_noise_p95'), sort_keys=True)}`",
            f"Coverage: `{json.dumps(paired.get('coverage'), sort_keys=True)}`",
            "",
            "## Evidence gates",
            "",
            *[f"- {name}: `{value}`" for name, value in gates.items()],
            "",
            "Positive deltas/improvements favor arm A. In compare mode A=ctx and B=Pi.",
            "A/A mode estimates noise only; it is not a product comparison.",
            "No win is claimed without separate ctx and Pi A/A runs, improvement lower bounds above `max(3%, 2×noise)`, complete cgroup memory, sufficient samples, and independent raw-data review.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run paired ABBA ctx/Pi observations")
    parser.add_argument(
        "--mode", choices=("compare", "aa-ctx", "aa-pi"), default="compare"
    )
    parser.add_argument("--ctx-path", default="/usr/bin/ctx")
    parser.add_argument("--agent", default="executor")
    parser.add_argument("--ctx-provider", required=True)
    parser.add_argument("--pi-path", default="pi")
    parser.add_argument("--pi-provider", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--thinking", required=True)
    parser.add_argument("--system-prompt")
    parser.add_argument("--credential-scope-attested", action="store_true")
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path(__file__).with_name("data") / "agent_benchmark.jsonl",
    )
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument("--output-dir", type=Path, default=Path("results/paired"))
    parser.add_argument("--retain-frames", action="store_true")
    parser.add_argument("--no-memory", action="store_true")
    _add_price_arguments(parser)
    args = parser.parse_args()
    if args.repeat < 1 or args.warmup < 0 or args.timeout <= 0:
        parser.error("--repeat/--timeout must be positive and --warmup non-negative")
    if _provider_family(args.ctx_provider) != _provider_family(args.pi_provider):
        parser.error("ctx and Pi providers do not resolve to the same provider family")
    if args.mode == "compare" and not args.credential_scope_attested:
        parser.error("compare mode requires --credential-scope-attested")
    try:
        dataset = _dataset(args.dataset)
        price_table = _price_table(args)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(str(error))

    uses_ctx = args.mode != "aa-pi"
    uses_pi = args.mode != "aa-ctx"
    pi_runner = (
        PiJsonlRunner(
            pi_path=args.pi_path,
            provider=args.pi_provider,
            model=args.model,
            thinking=args.thinking,
            timeout=args.timeout,
            system_prompt=args.system_prompt,
            retain_frames=args.retain_frames,
        )
        if uses_pi
        else None
    )
    if uses_ctx:
        try:
            health = _preflight(args.ctx_path, [args.agent], args.timeout)
            ctx_identity = _runtime_identity(args.ctx_path, [args.agent], args.timeout)
            _validate_model_controls(
                ctx_identity, args.ctx_provider, args.model, args.thinking
            )
            ctx_binary = ctx_identity.get("binary")
            if (
                not isinstance(ctx_binary, dict)
                or not ctx_binary.get("sha256")
                or not ctx_binary.get("version")
            ):
                raise ValueError("ctx executable identity is incomplete")
            if not _runtime_contracts_complete(ctx_identity):
                raise ValueError("ctx agent runtime contracts are incomplete")
        except ValueError as error:
            parser.error(str(sanitize(str(error))))
    else:
        health = None
        ctx_identity = None
    pi_binary = _binary_identity(args.pi_path) if pi_runner is not None else None
    pi_runner_identity = pi_runner.identity() if pi_runner is not None else None
    pi_settings = (
        pi_runner_identity.get("settings")
        if isinstance(pi_runner_identity, dict)
        else None
    )
    if pi_runner is not None and (
        not isinstance(pi_binary, dict)
        or not pi_binary.get("sha256")
        or not pi_binary.get("version")
    ):
        parser.error("Pi executable identity is incomplete")
    if pi_runner is not None and (
        not isinstance(pi_settings, dict) or not pi_settings.get("complete")
    ):
        parser.error("Pi settings identity is incomplete")
    ctx_runtime_snapshots_pre = (
        {
            "mount": _unit_runtime_snapshot("cortexfs.service", args.timeout),
            "agents": {
                args.agent: _unit_runtime_snapshot(
                    f"cortexfs-agent@{args.agent}.service", args.timeout
                )
            },
        }
        if uses_ctx
        else None
    )
    workload = {
        "dataset": _dataset_identity(args.dataset, dataset),
        "provider": _provider_family(args.pi_provider),
        "model": args.model,
        "thinking": args.thinking,
        "task_protocol": "single-prompt-final-answer-v1",
        "observation_plan": {
            "schedule": "ABBA",
            "pairs_per_task_epoch": 2,
            "epochs": args.repeat,
            "warmup_per_runtime": args.warmup,
        },
    }
    treatment = {
        "mode": args.mode,
        "ctx_scope": "single-agent-endpoint",
        "token_metric_semantics": "visible-input-output-v1",
        "ctx": {"agent": args.agent, "identity": ctx_identity},
        "pi": {
            "runner": pi_runner_identity,
            "binary": pi_binary,
        }
        if pi_runner is not None
        else None,
        "retain_frames": args.retain_frames,
        "credential_scope_attested": args.credential_scope_attested,
        "ctx_memory": "disabled"
        if args.no_memory
        else "systemd-unit-accounting-incomplete",
        "pi_memory": "20ms-process-tree-sampling-incomplete",
        "price_table": {
            "source": price_table.source,
            "input_usd_per_million": price_table.input_usd_per_million,
            "output_usd_per_million": price_table.output_usd_per_million,
            "cache_read_usd_per_million": price_table.cache_read_usd_per_million,
            "cache_write_usd_per_million": price_table.cache_write_usd_per_million,
        }
        if price_table is not None
        else None,
        "benchmark_source": _benchmark_source_identity(
            ["agents.py", "paired.py", "pi.py", "pi_system.py", "system.py"]
        ),
    }
    warmups = asyncio.run(
        _run_warmups(
            mode=args.mode,
            count=args.warmup,
            ctx_path=args.ctx_path,
            agent_name=args.agent,
            pi_runner=pi_runner,
            sample=dataset[0],
            timeout=args.timeout,
            retain_frames=args.retain_frames,
            price_table=price_table,
        )
    )
    samples = asyncio.run(
        _run_schedule(
            mode=args.mode,
            ctx_path=args.ctx_path,
            agent_name=args.agent,
            pi_runner=pi_runner,
            dataset=dataset,
            repeat=args.repeat,
            timeout=args.timeout,
            retain_frames=args.retain_frames,
            measure_memory=not args.no_memory,
            price_table=price_table,
        )
    )
    pairs = _pair_records(samples)
    post_ctx_identity = (
        _runtime_identity(args.ctx_path, [args.agent], args.timeout)
        if uses_ctx
        else None
    )
    ctx_runtime_snapshots_post = (
        {
            "mount": _unit_runtime_snapshot("cortexfs.service", args.timeout),
            "agents": {
                args.agent: _unit_runtime_snapshot(
                    f"cortexfs-agent@{args.agent}.service", args.timeout
                )
            },
        }
        if uses_ctx
        else None
    )
    post_pi_identity = (
        {
            "runner": pi_runner.identity(),
            "binary": _binary_identity(args.pi_path),
        }
        if pi_runner is not None
        else None
    )
    identity_stable = post_ctx_identity == ctx_identity and post_pi_identity == (
        {"runner": pi_runner_identity, "binary": pi_binary} if uses_pi else None
    )
    warmup_content = _jsonl_content(warmups)
    sample_content = _jsonl_content(samples)
    pair_content = _jsonl_content(pairs)
    fingerprint = _comparison_fingerprint(workload)
    all_records = [*warmups, *samples]
    complete_protocol_records = sum(
        bool(record.get("raw_frames_complete")) for record in all_records
    )
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    evidence_gates: dict[str, object] = {
        "strict_abba_order": True,
        "runtime_identity_stable": identity_stable,
        "runtime_snapshots_complete_and_stable": (
            True
            if not uses_ctx
            else bool(
                ctx_runtime_snapshots_pre
                and ctx_runtime_snapshots_post
                and _runtime_snapshots_stable(
                    ctx_runtime_snapshots_pre,
                    ctx_runtime_snapshots_post,
                )
            )
        ),
        "scope_is_multi_agent_team": False,
        "credential_scope_attested": args.mode != "compare"
        or args.credential_scope_attested,
        "credential_scope_verified": args.mode != "compare",
        "warmup_present": args.warmup > 0,
        "warmups_valid": _warmups_valid(warmups, args.mode, args.warmup),
        "ctx_contracts_complete": (
            True if ctx_identity is None else _runtime_contracts_complete(ctx_identity)
        ),
        "protocol_evidence_complete": complete_protocol_records == len(all_records),
        "protocol_evidence_complete_samples": complete_protocol_records,
        "usage_evidence_complete": _usage_evidence_complete(all_records),
        "process_group_cleanup_complete": _process_cleanup_complete(all_records),
        "runtime_process_identity_complete": (
            _runtime_process_identity_complete(all_records)
        ),
        "workspace_equivalent": args.mode != "compare",
        "system_prompt_equivalent": args.mode != "compare",
        "token_semantics_equivalent": True,
        "tools_equivalent": args.mode != "compare"
        or (ctx_identity is not None and _runtime_has_no_visible_tools(ctx_identity)),
        "complete_cgroup_memory": _memory_evidence_complete(all_records),
        "immutable_artifacts": True,
        "independent_review": False,
    }
    evidence_gates["all_pass"] = all(bool(value) for value in evidence_gates.values())
    summary = _sanitized_dict(
        {
            "run_id": run_id,
            "mode": args.mode,
            "workload": workload,
            "treatment": treatment,
            "runtime_identity_post": {
                "ctx": post_ctx_identity,
                "pi": post_pi_identity,
            },
            "ctx_runtime_process_snapshots": {
                "pre": ctx_runtime_snapshots_pre,
                "post": ctx_runtime_snapshots_post,
            },
            "fingerprint": fingerprint,
            "treatment_fingerprint": _comparison_fingerprint(treatment),
            "experiment": {
                "schedule": "ABBA",
                "bootstrap_seed": BOOTSTRAP_SEED,
                "warmup_per_runtime": args.warmup,
            },
            "health": health,
            "arm_a": _aggregate([record for record in samples if record["arm"] == "A"]),
            "arm_b": _aggregate([record for record in samples if record["arm"] == "B"]),
            "paired": _paired_metrics(pairs, args.mode),
            "artifacts": {
                "warmups_jsonl_sha256": hashlib.sha256(
                    warmup_content.encode()
                ).hexdigest(),
                "samples_jsonl_sha256": hashlib.sha256(
                    sample_content.encode()
                ).hexdigest(),
                "pairs_jsonl_sha256": hashlib.sha256(pair_content.encode()).hexdigest(),
                "immutable": True,
            },
            "evidence_gates": evidence_gates,
            "review": {"independent": False, "verdict": "pending"},
            "evidence_status": _evidence_status(evidence_gates),
        },
        128 * 1024,
    )
    try:
        print(
            _publish_output(
                args.output_dir,
                run_id,
                {
                    "warmups.jsonl": warmup_content,
                    "samples.jsonl": sample_content,
                    "pairs.jsonl": pair_content,
                },
                summary,
                _report(summary),
            )
        )
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot publish paired benchmark: {sanitize(str(error))}")
    _require_cleanup(
        [record for record in [*warmups, *samples] if record.get("runtime") == "ctx"]
    )


if __name__ == "__main__":
    main()
