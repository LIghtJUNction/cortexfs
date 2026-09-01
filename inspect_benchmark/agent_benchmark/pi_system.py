from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import shutil
import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .agents import record_process_cleanup_complete, sanitize
from .pi import PiJsonlRunner, PiUsage, _pi_environment
from .system import (
    _add_baseline_arguments,
    _aggregate,
    _benchmark_source_identity,
    _comparison_fingerprint,
    _dataset,
    _dataset_identity,
    _format_distribution,
    _hash_executable,
    _jsonl_content,
    _observation_metadata,
    _publish_output,
    _sanitized_dict,
    _validated_comparison,
)


@dataclass(frozen=True)
class ApiPriceTable:
    source: str
    input_usd_per_million: float
    output_usd_per_million: float
    cache_read_usd_per_million: float = 0.0
    cache_write_usd_per_million: float = 0.0

    def equivalent_cost(self, usage: PiUsage) -> float:
        weighted = (
            usage.input_tokens * self.input_usd_per_million
            + usage.output_tokens * self.output_usd_per_million
            + usage.cache_read_tokens * self.cache_read_usd_per_million
            + usage.cache_write_tokens * self.cache_write_usd_per_million
        )
        return weighted / 1_000_000


def _price_table(args: argparse.Namespace) -> ApiPriceTable | None:
    values = (
        args.input_usd_per_million,
        args.output_usd_per_million,
        args.cache_read_usd_per_million,
        args.cache_write_usd_per_million,
    )
    if all(value is None for value in values):
        return None
    if args.input_usd_per_million is None or args.output_usd_per_million is None:
        raise ValueError("input and output prices are required together")
    if not args.price_source:
        raise ValueError("--price-source is required with API-equivalent prices")
    normalized = tuple(0.0 if value is None else value for value in values)
    if any(value < 0 for value in normalized):
        raise ValueError("API-equivalent prices must be non-negative")
    return ApiPriceTable(args.price_source, *normalized)


def _provider_family(provider: str) -> str:
    return "codex" if provider in {"codex", "openai-codex"} else provider


def _binary_identity(pi_path: str) -> dict[str, object]:
    resolved = shutil.which(pi_path)
    if resolved is None:
        return {"requested_path": pi_path, "resolved_path": None, "version": None}
    path = Path(resolved).resolve()
    try:
        digest, size = _hash_executable(path)
    except OSError:
        digest = None
        size = 0
    environment = _pi_environment()
    try:
        result = subprocess.run(
            [str(path), "--version"],
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
            env=environment,
        )
    except (OSError, subprocess.TimeoutExpired):
        version = None
    else:
        version = result.stdout.strip() if result.returncode == 0 else None
    return {
        "requested_path": pi_path,
        "resolved_path": str(path),
        "sha256": digest,
        "bytes": size,
        "version": version,
    }


async def _pi_observation(
    *,
    runner: PiJsonlRunner,
    sample: Mapping[str, object],
    sample_index: int,
    epoch: int,
    price_table: ApiPriceTable | None,
) -> dict[str, Any]:
    result = await runner.run(str(sample["prompt"]))
    usage = result.usage
    equivalent_cost = (
        price_table.equivalent_cost(usage)
        if price_table is not None and result.usage_available
        else None
    )
    value = result.metadata()
    value.update(
        {
            "runtime": "pi",
            **_observation_metadata(
                sample,
                sample_index,
                epoch,
                result.completion,
                result.runtime_success,
            ),
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "comparable_tokens": usage.input_tokens + usage.output_tokens
            if result.usage_available
            else None,
            "usage_semantics": "visible-input-output-v1",
            "cache_read_tokens": usage.cache_read_tokens,
            "cache_write_tokens": usage.cache_write_tokens,
            "reasoning_tokens": usage.reasoning_tokens,
            "total_tokens": usage.total_tokens,
            "provider_catalog_cost_usd": usage.provider_catalog_cost_usd
            if result.usage_available
            else None,
            "api_equivalent_cost_usd": equivalent_cost,
            "memory_complete": result.memory_complete,
        }
    )
    return _sanitized_dict(value, 256 * 1024)


async def _samples(
    *,
    runner: PiJsonlRunner,
    dataset: Sequence[dict[str, Any]],
    repeat: int,
    price_table: ApiPriceTable | None,
) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for epoch in range(repeat):
        for index, sample in enumerate(dataset):
            records.append(
                await _pi_observation(
                    runner=runner,
                    sample=sample,
                    sample_index=index,
                    epoch=epoch,
                    price_table=price_table,
                )
            )
    return records


def _report(summary: dict[str, Any]) -> str:
    metrics = summary["overall"]
    latency = metrics["latency_ms"]
    ttft = metrics["ttft_ms"]
    rss = metrics["peak_rss_bytes"]
    lines = [
        "# Pi Coding Agent Baseline",
        "",
        f"Run: `{summary['run_id']}`",
        f"Workload fingerprint: `{summary['fingerprint']}`",
        f"Treatment fingerprint: `{summary['treatment_fingerprint']}`",
        "",
        "| Requests | Runtime success | Exact accuracy | Latency mean/p50/p95 | TTFT mean/p50/p95 | Peak RSS mean/p50/p95 | Usage coverage |",
        "| ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        f"| {metrics['requests']} | {metrics['runtime_success_rate']:.1%} | "
        f"{metrics['exact_accuracy']:.1%} | "
        f"{_format_distribution(latency, suffix=' ms')} | "
        f"{_format_distribution(ttft, suffix=' ms')} | "
        f"{_format_distribution(rss, decimals=0, suffix=' B')} | "
        f"{metrics['token_samples']}/{metrics['successful_samples']} |",
        "",
        "## Usage and cost",
        "",
        f"- Provider-total tokens across all attempts: `{metrics['total_tokens']}`",
        f"- Comparable visible input+output tokens: `{metrics['visible_input_output_tokens']}`",
        f"- Tokens/success: `{metrics['tokens_per_success']}`",
        f"- Provider-catalog cost: `{metrics['provider_catalog_cost_usd']}` USD",
        f"- Versioned API-equivalent cost: `{metrics['api_equivalent_cost_usd']}` USD",
        "- Attributable subscription cost: `n/a`",
        f"- Protocol frame coverage: `{summary['artifacts']['raw_frames_complete_samples']}/{metrics['requests']}`",
        f"- Samples SHA-256: `{summary['artifacts']['samples_jsonl_sha256']}`",
        "- Peak RSS is sampled and descriptive; complete memory coverage is `0` until cgroup evidence exists.",
        "",
        "## Evidence status",
        "",
        f"`{summary['evidence_status']}`",
        f"Gates: `{json.dumps(summary.get('evidence_gates', {}), sort_keys=True)}`",
        "",
        "This runner does not claim a performance win without paired A/A noise, AB/BA or ABBA ordering, sufficient samples, and independent raw-data review.",
        "",
        "## Errors",
        "",
        "```json",
        json.dumps(metrics["errors"], ensure_ascii=False, indent=2),
        "```",
        "",
    ]
    return "\n".join(lines)


def _add_price_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--price-source")
    parser.add_argument("--input-usd-per-million", type=float)
    parser.add_argument("--output-usd-per-million", type=float)
    parser.add_argument("--cache-read-usd-per-million", type=float)
    parser.add_argument("--cache-write-usd-per-million", type=float)


def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark Pi through JSONL mode")
    parser.add_argument("--pi-path", default="pi")
    parser.add_argument("--provider", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--thinking", required=True)
    parser.add_argument("--system-prompt")
    _add_baseline_arguments(parser, Path("results"))
    _add_price_arguments(parser)
    args = parser.parse_args()
    if args.repeat < 1 or args.timeout <= 0:
        parser.error("--repeat and --timeout must be positive")
    try:
        dataset = _dataset(args.dataset)
        price_table = _price_table(args)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(str(error))

    runner = PiJsonlRunner(
        pi_path=args.pi_path,
        provider=args.provider,
        model=args.model,
        thinking=args.thinking,
        timeout=args.timeout,
        system_prompt=args.system_prompt,
        retain_frames=args.retain_frames,
    )
    workload = {
        "dataset": _dataset_identity(args.dataset, dataset),
        "repeat": args.repeat,
        "timeout": args.timeout,
        "provider": _provider_family(args.provider),
        "model": args.model,
        "thinking": args.thinking,
        "task_protocol": "single-prompt-final-answer-v1",
        "observation_plan": {"subjects_per_task": 1, "repeat_per_subject": args.repeat},
    }
    binary_identity = _binary_identity(args.pi_path)
    runner_identity = runner.identity()
    settings_identity = runner_identity.get("settings")
    if not binary_identity.get("sha256") or not binary_identity.get("version"):
        parser.error("Pi executable identity is incomplete")
    if not isinstance(settings_identity, dict) or not settings_identity.get("complete"):
        parser.error("Pi settings identity is incomplete")
    treatment = {
        "runtime": "pi-jsonl-cold-cli",
        "runner": runner_identity,
        "binary": binary_identity,
        "retain_frames": args.retain_frames,
        "price_table": asdict(price_table) if price_table is not None else None,
        "token_metric_semantics": "visible-input-output-v1",
        "benchmark_source": _benchmark_source_identity(
            ["agents.py", "pi.py", "pi_system.py", "system.py"]
        ),
    }
    fingerprint = _comparison_fingerprint(workload)
    treatment_fingerprint = _comparison_fingerprint(treatment)
    try:
        comparison = _validated_comparison(
            args.compare_summary, fingerprint, args.allow_mismatch
        )
    except ValueError as error:
        parser.error(str(error))

    samples = asyncio.run(
        _samples(
            runner=runner,
            dataset=dataset,
            repeat=args.repeat,
            price_table=price_table,
        )
    )
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    metrics = _aggregate(samples)
    post_runner_identity = runner.identity()
    post_binary_identity = _binary_identity(args.pi_path)
    identity_stable = (
        post_runner_identity == runner_identity
        and post_binary_identity == binary_identity
    )
    evidence_status = (
        "inconclusive: fewer than 100 successful samples; p95 is descriptive"
        if metrics["successful_samples"] < 100
        else "inconclusive: paired A/A noise and independent review are still required"
    )
    content = _jsonl_content(samples)
    summary = _sanitized_dict(
        {
            "run_id": run_id,
            "workload": workload,
            "treatment": treatment,
            "runtime_identity_post": {
                "runner": post_runner_identity,
                "binary": post_binary_identity,
            },
            "fingerprint": fingerprint,
            "treatment_fingerprint": treatment_fingerprint,
            "comparison": comparison,
            "overall": metrics,
            "evidence_status": evidence_status,
            "review": {"independent": False, "verdict": "pending"},
            "evidence_gates": {
                "warmup_present": False,
                "runtime_identity_stable": identity_stable,
                "process_group_cleanup_complete": all(
                    record_process_cleanup_complete(record) for record in samples
                ),
                "protocol_evidence_complete": sum(
                    bool(record.get("raw_frames_complete")) for record in samples
                )
                == metrics["requests"],
                "complete_cgroup_memory": bool(samples)
                and all(bool(record.get("memory_complete")) for record in samples),
                "immutable_artifacts": True,
                "independent_review": False,
                "all_pass": False,
            },
            "artifacts": {
                "samples_jsonl_sha256": hashlib.sha256(content.encode()).hexdigest(),
                "samples_jsonl_bytes": len(content.encode()),
                "raw_frames_complete_samples": sum(
                    bool(record.get("raw_frames_complete")) for record in samples
                ),
                "immutable": True,
            },
        },
        64 * 1024,
    )
    try:
        print(
            _publish_output(
                args.output_dir,
                run_id,
                {"samples.jsonl": content},
                summary,
                _report(summary),
            )
        )
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot publish Pi benchmark output: {sanitize(str(error))}")


if __name__ == "__main__":
    main()
