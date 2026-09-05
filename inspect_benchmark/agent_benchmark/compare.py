from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import tempfile
from collections.abc import Mapping
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, BinaryIO

from .agents import sanitize
from .system import OutputDirectory, _nonnegative_number, _sanitized_dict

MAX_SUMMARY_BYTES = 1024 * 1024
MAX_REVIEW_BYTES = 1024 * 1024
REVIEW_NAMESPACE = "cortexfs-benchmark-review-v1"
SSH_KEYGEN = "/usr/bin/ssh-keygen"
PRINCIPAL_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._@+-]{0,127}$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")


class _DuplicateKeyError(ValueError):
    pass


def _read_regular_bytes(path: Path, limit: int, kind: str) -> bytes:
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ValueError(f"cannot open {kind}: {path}") from error
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError(f"{kind} is not a bounded regular file: {path}")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            content = source.read(limit + 1)
        if len(content) > limit:
            raise ValueError(f"{kind} exceeds {limit} bytes: {path}")
        return content
    except OSError as error:
        raise ValueError(f"cannot read {kind}: {path}") from error
    finally:
        os.close(descriptor)


def _decode_object(content: bytes, path: Path, *, strict: bool) -> dict[str, Any]:
    def pairs_hook(pairs: list[tuple[str, object]]) -> dict[str, object]:
        value: dict[str, object] = {}
        for key, item in pairs:
            if key in value:
                raise _DuplicateKeyError(key)
            value[key] = item
        return value

    try:
        parsed = json.loads(
            content.decode("utf-8"),
            object_pairs_hook=pairs_hook if strict else None,
        )
    except _DuplicateKeyError as error:
        raise ValueError(f"duplicate JSON key in {path}: {error}") from error
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON object: {path}") from error
    if not isinstance(parsed, dict):
        raise ValueError(f"JSON value must be an object: {path}")
    return parsed


def _load_summary_with_hash(path: Path) -> tuple[dict[str, Any], str]:
    content = _read_regular_bytes(path, MAX_SUMMARY_BYTES, "summary")
    return _decode_object(content, path, strict=True), hashlib.sha256(
        content
    ).hexdigest()


def _load_summary(path: Path) -> dict[str, Any]:
    return _load_summary_with_hash(path)[0]


def _required_string(value: object, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"missing review binding: {field}")
    return value


def _summary_binding(
    summary: Mapping[str, object], summary_sha256: str, label: str
) -> dict[str, object]:
    artifacts = _mapping(summary.get("artifacts"), f"{label}.artifacts")
    hashes = {
        name: value for name, value in artifacts.items() if name.endswith("_sha256")
    }
    if not hashes or any(
        not isinstance(value, str) or not SHA256_PATTERN.fullmatch(value)
        for value in hashes.values()
    ):
        raise ValueError(f"{label} summary has incomplete artifact hashes")
    return {
        "summary_sha256": summary_sha256,
        "run_id": _required_string(summary.get("run_id"), f"{label}.run_id"),
        "fingerprint": _required_string(
            summary.get("fingerprint"), f"{label}.fingerprint"
        ),
        "treatment_fingerprint": _required_string(
            summary.get("treatment_fingerprint"),
            f"{label}.treatment_fingerprint",
        ),
        "artifacts": dict(sorted(hashes.items())),
    }


def _review_statement(
    content: bytes,
    path: Path,
    *,
    reviewer_principal: str,
    operator_principal: str,
    ctx_binding: Mapping[str, object],
    pi_binding: Mapping[str, object],
) -> dict[str, Any]:
    statement = _decode_object(content, path, strict=True)
    expected_keys = {
        "schema",
        "namespace",
        "reviewer_principal",
        "operator_principal",
        "verdict",
        "ctx",
        "pi",
    }
    if set(statement) != expected_keys:
        raise ValueError("review statement has unexpected or missing fields")
    if statement.get("schema") != "cortexfs.benchmark-review/v1":
        raise ValueError("review statement schema mismatch")
    if statement.get("namespace") != REVIEW_NAMESPACE:
        raise ValueError("review statement namespace mismatch")
    if statement.get("reviewer_principal") != reviewer_principal:
        raise ValueError("review statement principal mismatch")
    if statement.get("operator_principal") != operator_principal:
        raise ValueError("review statement operator principal mismatch")
    if statement.get("verdict") != "approve":
        raise ValueError("review statement verdict must be approve")
    for label, expected in (("ctx", ctx_binding), ("pi", pi_binding)):
        actual = statement.get(label)
        if not isinstance(actual, dict) or actual != expected:
            raise ValueError(f"review statement {label} binding mismatch")
    return statement


def _review_temporary_file(name: str, content: bytes) -> BinaryIO:
    configured_root = os.environ.get("XDG_STATE_HOME")
    state_root = (
        Path(configured_root) if configured_root else Path.home() / ".local" / "state"
    )
    review_root = state_root / "cortexfs-review-verifier"
    temporary: BinaryIO | None = None
    try:
        review_root.mkdir(mode=0o700, parents=True, exist_ok=True)
        metadata = review_root.lstat()
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or stat.S_ISLNK(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or metadata.st_mode & 0o077
        ):
            raise ValueError("review verification state directory is not private")
        temporary = tempfile.TemporaryFile(
            mode="w+b", prefix=f"{name}-", dir=review_root
        )
        view = memoryview(content)
        while view:
            written = temporary.write(view)
            if written is None or written <= 0:
                raise OSError("short review evidence write")
            view = view[written:]
        temporary.flush()
        temporary.seek(0)
        return temporary
    except OSError as error:
        if temporary is not None:
            temporary.close()
        raise ValueError("cannot create private review verification file") from error


def _verify_signature(
    statement: bytes,
    signature: bytes,
    allowed_signers: bytes,
    reviewer_principal: str,
) -> None:
    with (
        _review_temporary_file("signature", signature) as signature_file,
        _review_temporary_file("allowed-signers", allowed_signers) as allowed_file,
    ):
        signature_descriptor = signature_file.fileno()
        allowed_descriptor = allowed_file.fileno()
        try:
            completed = subprocess.run(
                [
                    SSH_KEYGEN,
                    "-Y",
                    "verify",
                    "-f",
                    f"/proc/self/fd/{allowed_descriptor}",
                    "-I",
                    reviewer_principal,
                    "-n",
                    REVIEW_NAMESPACE,
                    "-s",
                    f"/proc/self/fd/{signature_descriptor}",
                ],
                input=statement,
                capture_output=True,
                pass_fds=(allowed_descriptor, signature_descriptor),
                timeout=10,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ValueError("review signature verification could not run") from error
    if completed.returncode != 0:
        raise ValueError("review signature verification failed")


def _verify_external_review(
    *,
    statement_path: Path,
    signature_path: Path,
    allowed_signers_path: Path,
    reviewer_principal: str,
    operator_principal: str,
    ctx: Mapping[str, object],
    pi: Mapping[str, object],
    ctx_sha256: str,
    pi_sha256: str,
) -> dict[str, object]:
    if not PRINCIPAL_PATTERN.fullmatch(reviewer_principal):
        raise ValueError("invalid reviewer principal")
    if not PRINCIPAL_PATTERN.fullmatch(operator_principal):
        raise ValueError("invalid operator principal")
    if reviewer_principal == operator_principal:
        raise ValueError("reviewer principal must differ from operator principal")
    statement = _read_regular_bytes(
        statement_path, MAX_REVIEW_BYTES, "review statement"
    )
    signature = _read_regular_bytes(
        signature_path, MAX_REVIEW_BYTES, "review signature"
    )
    allowed_signers = _read_regular_bytes(
        allowed_signers_path, MAX_REVIEW_BYTES, "allowed signers"
    )
    parsed = _review_statement(
        statement,
        statement_path,
        reviewer_principal=reviewer_principal,
        operator_principal=operator_principal,
        ctx_binding=_summary_binding(ctx, ctx_sha256, "ctx"),
        pi_binding=_summary_binding(pi, pi_sha256, "pi"),
    )
    _verify_signature(statement, signature, allowed_signers, reviewer_principal)
    return {
        "provided": True,
        "verified": True,
        "namespace": REVIEW_NAMESPACE,
        "reviewer_principal": reviewer_principal,
        "operator_principal": operator_principal,
        "verdict": parsed["verdict"],
        "ctx_summary_sha256": ctx_sha256,
        "pi_summary_sha256": pi_sha256,
        "statement_sha256": hashlib.sha256(statement).hexdigest(),
        "signature_sha256": hashlib.sha256(signature).hexdigest(),
        "allowed_signers_sha256": hashlib.sha256(allowed_signers).hexdigest(),
    }


def _mapping(value: object, field: str) -> Mapping[str, object]:
    if not isinstance(value, dict):
        raise ValueError(f"missing metric object: {field}")
    return value


def _metric(summary: Mapping[str, object], *path: str) -> float:
    value: object = summary
    for part in path:
        value = _mapping(value, ".".join(path)).get(part)
    return _nonnegative_number(value, ".".join(path))


def _optional_metric(summary: Mapping[str, object], *path: str) -> float | None:
    try:
        return _metric(summary, *path)
    except ValueError:
        return None


def _outcome(ctx: float, pi: float, *, lower_is_better: bool) -> str:
    if ctx == pi:
        return "tie"
    wins = ctx < pi if lower_is_better else ctx > pi
    return "ctx" if wins else "pi"


def _comparison(
    ctx: Mapping[str, object], pi: Mapping[str, object]
) -> dict[str, object]:
    ctx_score = _metric(ctx, "overall", "exact_accuracy")
    pi_score = _metric(pi, "overall", "exact_accuracy")
    ctx_latency = _metric(ctx, "overall", "latency_ms", "p95")
    pi_latency = _metric(pi, "overall", "latency_ms", "p95")
    ctx_rss = _optional_metric(ctx, "overall", "peak_rss_bytes", "p95")
    pi_rss = _optional_metric(pi, "overall", "peak_rss_bytes", "p95")
    ctx_memory_complete = _metric(ctx, "overall", "complete_memory_samples")
    pi_memory_complete = _metric(pi, "overall", "complete_memory_samples")
    ctx_requests = _metric(ctx, "overall", "requests")
    pi_requests = _metric(pi, "overall", "requests")
    ctx_cost = _optional_metric(ctx, "overall", "api_equivalent_cost_per_success_usd")
    pi_cost = _optional_metric(pi, "overall", "api_equivalent_cost_per_success_usd")
    ctx_treatment = _mapping(ctx.get("treatment"), "ctx.treatment")
    pi_treatment = _mapping(pi.get("treatment"), "pi.treatment")
    same_price_table = ctx_treatment.get("price_table") == pi_treatment.get(
        "price_table"
    )
    cost_basis = "versioned_api_equivalent_cost_per_success_usd"
    if ctx_cost is None or pi_cost is None or not same_price_table:
        ctx_usage = _metric(ctx, "overall", "token_samples")
        pi_usage = _metric(pi, "overall", "token_samples")
        if ctx_usage < ctx_requests or pi_usage < pi_requests:
            raise ValueError("cost comparison requires complete usage coverage")
        if ctx_treatment.get("token_metric_semantics") != pi_treatment.get(
            "token_metric_semantics"
        ):
            raise ValueError("cost comparison requires equivalent token semantics")
        ctx_cost = _metric(ctx, "overall", "visible_tokens_per_success")
        pi_cost = _metric(pi, "overall", "visible_tokens_per_success")
        cost_basis = "visible_input_output_tokens_per_success"

    memory_coverage = (
        ctx_memory_complete >= ctx_requests
        and pi_memory_complete >= pi_requests
        and ctx_rss is not None
        and pi_rss is not None
    )
    quality_outcome = _outcome(ctx_score, pi_score, lower_is_better=False)
    speed_outcome = _outcome(ctx_latency, pi_latency, lower_is_better=True)
    memory_outcome = (
        _outcome(ctx_rss, pi_rss, lower_is_better=True)
        if memory_coverage and ctx_rss is not None and pi_rss is not None
        else "inconclusive"
    )
    cost_outcome = _outcome(ctx_cost, pi_cost, lower_is_better=True)
    return {
        "quality": {
            "metric": "exact_accuracy",
            "ctx": ctx_score,
            "pi": pi_score,
            "delta": ctx_score - pi_score,
            "point_outcome": quality_outcome,
        },
        "speed": {
            "metric": "latency_p95_ms",
            "ctx": ctx_latency,
            "pi": pi_latency,
            "improvement": 1 - ctx_latency / pi_latency if pi_latency else None,
            "point_outcome": speed_outcome,
        },
        "memory": {
            "metric": "complete_stack_peak_rss_p95_bytes",
            "ctx": ctx_rss,
            "pi": pi_rss,
            "complete_coverage": memory_coverage,
            "point_outcome": memory_outcome,
        },
        "cost": {
            "metric": cost_basis,
            "ctx": ctx_cost,
            "pi": pi_cost,
            "improvement": 1 - ctx_cost / pi_cost if pi_cost else None,
            "point_outcome": cost_outcome,
            "attributable_subscription_cost_usd": None,
        },
    }


def _evidence_gates(
    ctx: Mapping[str, object],
    pi: Mapping[str, object],
    points: Mapping[str, object],
    *,
    independent_review: bool = False,
) -> dict[str, object]:
    ctx_experiment = ctx.get("experiment")
    pi_experiment = pi.get("experiment")
    paired_order = (
        isinstance(ctx_experiment, dict)
        and isinstance(pi_experiment, dict)
        and ctx_experiment.get("schedule") in {"ABBA", "AB/BA"}
        and ctx_experiment.get("schedule") == pi_experiment.get("schedule")
    )
    aa_noise = (
        isinstance(ctx_experiment, dict)
        and isinstance(pi_experiment, dict)
        and isinstance(ctx_experiment.get("aa_noise"), dict)
        and isinstance(pi_experiment.get("aa_noise"), dict)
    )
    point_results = [
        points.get(name) for name in ("quality", "speed", "memory", "cost")
    ]
    point_wins = all(
        isinstance(point, dict) and point.get("point_outcome") == "ctx"
        for point in point_results
    )
    return {
        "paired_order": paired_order,
        "aa_noise": aa_noise,
        "independent_raw_data_review": independent_review,
        "all_four_point_estimates_favor_ctx": point_wins,
        "performance_threshold": "not evaluated without reconstructing paired raw samples and metric noise",
        "summary_only_evidence": True,
        "all_pass": False,
    }


def _report(result: Mapping[str, object]) -> str:
    points = _mapping(result["point_estimates"], "point_estimates")
    gates = _mapping(result["evidence_gates"], "evidence_gates")
    lines = [
        "# CortexFS vs Pi comparison",
        "",
        f"Workload fingerprint: `{result['fingerprint']}`",
        f"Verdict: **{result['verdict']}**",
        "",
        "| Dimension | Metric | ctx | Pi | Point outcome |",
        "| --- | --- | ---: | ---: | --- |",
    ]
    for name in ("quality", "speed", "memory", "cost"):
        point = _mapping(points[name], name)
        lines.append(
            f"| {name} | {point['metric']} | {point['ctx']} | {point['pi']} | {point['point_outcome']} |"
        )
    lines.extend(
        [
            "",
            "## Evidence gates",
            "",
            *[f"- {name}: `{value}`" for name, value in gates.items()],
            "",
            "A point estimate is not a win claim. The verdict stays inconclusive until raw paired samples reconstruct every metric, improvement lower bounds exceed `max(3%, 2×noise)`, complete-stack memory coverage exists, and an independent reviewer signs the evidence.",
            "",
            "Actual attributable subscription cost remains `n/a`.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare ctx and Pi summary evidence")
    parser.add_argument("--ctx-summary", type=Path, required=True)
    parser.add_argument("--pi-summary", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path("results/compare"))
    parser.add_argument("--require-all-gates", action="store_true")
    parser.add_argument("--review-statement", type=Path)
    parser.add_argument("--review-signature", type=Path)
    parser.add_argument("--allowed-signers", type=Path)
    parser.add_argument("--reviewer-principal")
    parser.add_argument("--operator-principal")
    args = parser.parse_args()
    try:
        review_inputs = (
            args.review_statement,
            args.review_signature,
            args.allowed_signers,
            args.reviewer_principal,
            args.operator_principal,
        )
        if any(value is not None for value in review_inputs) and not all(
            value is not None for value in review_inputs
        ):
            raise ValueError(
                "external review requires statement, signature, allowed signers, reviewer principal, and operator principal"
            )
        ctx, ctx_sha256 = _load_summary_with_hash(args.ctx_summary)
        pi, pi_sha256 = _load_summary_with_hash(args.pi_summary)
        ctx_fingerprint = ctx.get("fingerprint")
        pi_fingerprint = pi.get("fingerprint")
        if not isinstance(ctx_fingerprint, str) or ctx_fingerprint != pi_fingerprint:
            raise ValueError("ctx and Pi workload fingerprints differ")
        review: dict[str, object] = {
            "provided": False,
            "verified": False,
            "namespace": REVIEW_NAMESPACE,
        }
        if all(value is not None for value in review_inputs):
            review = _verify_external_review(
                statement_path=args.review_statement,
                signature_path=args.review_signature,
                allowed_signers_path=args.allowed_signers,
                reviewer_principal=args.reviewer_principal,
                operator_principal=args.operator_principal,
                ctx=ctx,
                pi=pi,
                ctx_sha256=ctx_sha256,
                pi_sha256=pi_sha256,
            )
        points = _comparison(ctx, pi)
        gates = _evidence_gates(
            ctx,
            pi,
            points,
            independent_review=bool(review.get("verified")),
        )
    except ValueError as error:
        parser.error(str(sanitize(str(error))))
    result = _sanitized_dict(
        {
            "fingerprint": ctx_fingerprint,
            "ctx_run_id": ctx.get("run_id"),
            "pi_run_id": pi.get("run_id"),
            "point_estimates": points,
            "evidence_gates": gates,
            "review_verification": review,
            "verdict": "pass" if gates["all_pass"] else "inconclusive",
        },
        64 * 1024,
    )
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    try:
        with OutputDirectory.create(args.output_dir, run_id) as output:
            output.write(
                "comparison.json",
                json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            )
            output.write("comparison.md", _report(result))
            output.seal()
            print(output.path)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot publish comparison: {sanitize(str(error))}")
    if args.require_all_gates and not gates["all_pass"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
