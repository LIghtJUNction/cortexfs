# pyright: reportMissingImports=false
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

from agent_benchmark.compare import (
    REVIEW_NAMESPACE,
    SSH_KEYGEN,
    _comparison,
    _evidence_gates,
    _load_summary,
    _report,
    _review_statement,
    _summary_binding,
    _verify_external_review,
    main,
)


def _summary(
    *,
    score: float,
    latency: float,
    rss: float,
    tokens: float,
    memory_complete: int,
) -> dict[str, object]:
    return {
        "run_id": "run",
        "fingerprint": "same",
        "treatment_fingerprint": "treatment",
        "artifacts": {"samples_jsonl_sha256": "a" * 64},
        "overall": {
            "exact_accuracy": score,
            "latency_ms": {"p95": latency},
            "peak_rss_bytes": {"p95": rss},
            "complete_memory_samples": memory_complete,
            "requests": 10,
            "successful_samples": 10,
            "token_samples": 10,
            "tokens_per_success": tokens,
            "visible_tokens_per_success": tokens,
            "api_equivalent_cost_per_success_usd": None,
        },
        "treatment": {
            "price_table": None,
            "token_metric_semantics": "visible-input-output-v1",
        },
        "review": {"independent": False},
    }


def _review_material(root: Path) -> dict[str, object]:
    ctx = _summary(score=0.9, latency=10, rss=100, tokens=5, memory_complete=10)
    pi = _summary(score=0.8, latency=20, rss=200, tokens=10, memory_complete=10)
    ctx["run_id"] = "ctx-run"
    pi["run_id"] = "pi-run"
    ctx_path = root / "ctx.json"
    pi_path = root / "pi.json"
    ctx_path.write_text(json.dumps(ctx, sort_keys=True) + "\n", encoding="utf-8")
    pi_path.write_text(json.dumps(pi, sort_keys=True) + "\n", encoding="utf-8")
    ctx_sha = hashlib.sha256(ctx_path.read_bytes()).hexdigest()
    pi_sha = hashlib.sha256(pi_path.read_bytes()).hexdigest()
    reviewer = "reviewer@example"
    statement = {
        "schema": "cortexfs.benchmark-review/v1",
        "namespace": REVIEW_NAMESPACE,
        "reviewer_principal": reviewer,
        "operator_principal": "operator@example",
        "verdict": "approve",
        "ctx": _summary_binding(ctx, ctx_sha, "ctx"),
        "pi": _summary_binding(pi, pi_sha, "pi"),
    }
    statement_path = root / "statement.json"
    signature_path = root / "statement.sig"
    allowed_path = root / "allowed_signers"
    statement_path.write_text(
        json.dumps(statement, sort_keys=True) + "\n", encoding="utf-8"
    )
    signature_path.write_text("signature", encoding="utf-8")
    allowed_path.write_text("allowed", encoding="utf-8")
    return {
        "ctx": ctx,
        "pi": pi,
        "ctx_path": ctx_path,
        "pi_path": pi_path,
        "ctx_sha": ctx_sha,
        "pi_sha": pi_sha,
        "reviewer": reviewer,
        "statement": statement,
        "statement_path": statement_path,
        "signature_path": signature_path,
        "allowed_path": allowed_path,
    }


def _verify_material(material: dict[str, object]) -> dict[str, object]:
    return _verify_external_review(
        statement_path=material["statement_path"],  # type: ignore[arg-type]
        signature_path=material["signature_path"],  # type: ignore[arg-type]
        allowed_signers_path=material["allowed_path"],  # type: ignore[arg-type]
        reviewer_principal=material["reviewer"],  # type: ignore[arg-type]
        operator_principal="operator@example",
        ctx=material["ctx"],  # type: ignore[arg-type]
        pi=material["pi"],  # type: ignore[arg-type]
        ctx_sha256=material["ctx_sha"],  # type: ignore[arg-type]
        pi_sha256=material["pi_sha"],  # type: ignore[arg-type]
    )


def test_comparison_refuses_memory_point_win_without_complete_stack_coverage() -> None:
    ctx = _summary(score=0.9, latency=10, rss=100, tokens=5, memory_complete=0)
    pi = _summary(score=0.8, latency=20, rss=200, tokens=10, memory_complete=10)
    points = _comparison(ctx, pi)

    assert points["quality"]["point_outcome"] == "ctx"  # type: ignore[index]
    assert points["memory"]["point_outcome"] == "inconclusive"  # type: ignore[index]
    gates = _evidence_gates(ctx, pi, points)
    assert gates["all_four_point_estimates_favor_ctx"] is False
    assert gates["all_pass"] is False


def test_summary_only_evidence_cannot_pass_even_with_claimed_flags() -> None:
    ctx = _summary(score=0.9, latency=10, rss=100, tokens=5, memory_complete=10)
    pi = _summary(score=0.8, latency=20, rss=200, tokens=10, memory_complete=10)
    for summary in (ctx, pi):
        summary["experiment"] = {"schedule": "ABBA", "aa_noise": {"p95": 0.01}}
        summary["review"] = {"independent": True}
    gates = _evidence_gates(ctx, pi, _comparison(ctx, pi))

    assert gates["paired_order"] is True
    assert gates["aa_noise"] is True
    assert gates["independent_raw_data_review"] is False
    assert gates["summary_only_evidence"] is True
    assert gates["all_pass"] is False


def test_valid_external_review_with_patched_signature(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    result = _verify_material(material)
    assert result["verified"] is True
    assert (
        result["statement_sha256"]
        == hashlib.sha256(
            material["statement_path"].read_bytes()  # type: ignore[union-attr]
        ).hexdigest()
    )
    ctx = material["ctx"]
    pi = material["pi"]
    assert isinstance(ctx, dict) and isinstance(pi, dict)
    gates = _evidence_gates(
        ctx,
        pi,
        _comparison(ctx, pi),
        independent_review=bool(result["verified"]),
    )
    assert gates["independent_raw_data_review"] is True


def test_valid_detached_ed25519_signature_and_tamper_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    with tempfile.TemporaryDirectory(dir=tmp_path) as directory:
        root = Path(directory)
        state_root = root / "state"
        monkeypatch.setenv("XDG_STATE_HOME", str(state_root))
        material = _review_material(root)
        key = root / "review-key"
        subprocess.run(
            [SSH_KEYGEN, "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
            check=True,
            capture_output=True,
        )
        public = key.with_suffix(".pub").read_text(encoding="utf-8").split()
        allowed = material["allowed_path"]
        assert isinstance(allowed, Path)
        allowed.write_text(
            f"{material['reviewer']} {public[0]} {public[1]}\n", encoding="utf-8"
        )
        statement = material["statement_path"]
        assert isinstance(statement, Path)
        signature = Path(f"{statement}.sig")
        material["signature_path"] = signature
        subprocess.run(
            [
                SSH_KEYGEN,
                "-Y",
                "sign",
                "-f",
                str(key),
                "-n",
                REVIEW_NAMESPACE,
                str(statement),
            ],
            check=True,
            capture_output=True,
        )
        assert _verify_material(material)["verified"] is True
        allowed.write_text(
            f"different@example {public[0]} {public[1]}\n", encoding="utf-8"
        )
        with pytest.raises(ValueError, match="signature verification failed"):
            _verify_material(material)
        allowed.write_text(
            f"{material['reviewer']} {public[0]} {public[1]}\n", encoding="utf-8"
        )
        statement.write_bytes(statement.read_bytes() + b"\n")
        with pytest.raises(ValueError, match="signature verification failed"):
            _verify_material(material)
        assert list((state_root / "cortexfs-review-verifier").iterdir()) == []


def test_tampered_summary_binding_fails_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    ctx = material["ctx"]
    assert isinstance(ctx, dict)
    ctx["run_id"] = "tampered"
    with pytest.raises(ValueError, match="ctx binding mismatch"):
        _verify_material(material)


def test_reviewer_must_differ_from_operator(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    material["reviewer"] = "operator@example"
    statement = material["statement"]
    assert isinstance(statement, dict)
    statement["reviewer_principal"] = "operator@example"
    path = material["statement_path"]
    assert isinstance(path, Path)
    path.write_text(json.dumps(statement) + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="must differ"):
        _verify_material(material)


def test_wrong_reviewer_principal_fails_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    material["reviewer"] = "other@example"
    with pytest.raises(ValueError, match="principal mismatch"):
        _verify_material(material)


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("namespace", "wrong", "namespace mismatch"),
        ("verdict", "reject", "verdict must be approve"),
    ],
)
def test_statement_namespace_and_verdict_fail_closed(
    tmp_path: Path, field: str, value: str, message: str
) -> None:
    material = _review_material(tmp_path)
    statement = material["statement"]
    assert isinstance(statement, dict)
    statement[field] = value
    with pytest.raises(ValueError, match=message):
        _review_statement(
            (json.dumps(statement) + "\n").encode(),
            tmp_path / "statement.json",
            reviewer_principal="reviewer@example",
            operator_principal="operator@example",
            ctx_binding=statement["ctx"],  # type: ignore[arg-type]
            pi_binding=statement["pi"],  # type: ignore[arg-type]
        )


def test_statement_duplicate_keys_fail_closed(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="duplicate JSON key"):
        _review_statement(
            b'{"schema":"one","schema":"two"}',
            tmp_path / "statement.json",
            reviewer_principal="reviewer@example",
            operator_principal="operator@example",
            ctx_binding={},
            pi_binding={},
        )


def test_partial_review_flags_fail_before_loading_summaries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "compare",
            "--ctx-summary",
            "ctx.json",
            "--pi-summary",
            "pi.json",
            "--review-statement",
            "statement.json",
        ],
    )
    with pytest.raises(SystemExit) as error:
        main()
    assert error.value.code == 2


@pytest.mark.parametrize(
    "name",
    ("statement_path", "signature_path", "allowed_path"),
)
def test_review_evidence_symlinks_fail_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, name: str
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    path = material[name]
    assert isinstance(path, Path)
    target = tmp_path / f"{name}.target"
    target.write_bytes(path.read_bytes())
    path.unlink()
    path.symlink_to(target)
    with pytest.raises(ValueError, match="cannot open"):
        _verify_material(material)


def test_review_evidence_non_regular_file_fails_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    material = _review_material(tmp_path)
    monkeypatch.setattr(
        "agent_benchmark.compare._verify_signature", lambda *_args: None
    )
    signature = material["signature_path"]
    assert isinstance(signature, Path)
    signature.unlink()
    signature.mkdir()
    with pytest.raises(ValueError, match="not a bounded regular file"):
        _verify_material(material)


def test_cost_comparison_rejects_mismatched_token_semantics() -> None:
    ctx = _summary(score=0.8, latency=1.0, rss=10, tokens=10, memory_complete=10)
    pi = _summary(score=0.8, latency=1.0, rss=10, tokens=10, memory_complete=10)
    treatment = pi["treatment"]
    assert isinstance(treatment, dict)
    treatment.update({"token_metric_semantics": "provider-total-v1"})
    with pytest.raises(ValueError, match="token semantics"):
        _comparison(ctx, pi)


def test_cost_comparison_rejects_partial_usage_coverage() -> None:
    ctx = _summary(score=0.9, latency=10, rss=100, tokens=5, memory_complete=0)
    pi = _summary(score=0.8, latency=20, rss=200, tokens=10, memory_complete=0)
    overall = ctx["overall"]
    assert isinstance(overall, dict)
    overall["token_samples"] = 9
    with pytest.raises(ValueError, match="complete usage"):
        _comparison(ctx, pi)


def test_load_summary_is_bounded_and_requires_an_object(tmp_path: Path) -> None:
    summary = tmp_path / "summary.json"
    summary.write_text(json.dumps({"fingerprint": "x"}), encoding="utf-8")
    assert _load_summary(summary)["fingerprint"] == "x"

    summary.write_text("[]", encoding="utf-8")
    with pytest.raises(ValueError, match="must be an object"):
        _load_summary(summary)

    summary.write_text('{"fingerprint":"x","fingerprint":"y"}', encoding="utf-8")
    with pytest.raises(ValueError, match="duplicate JSON key"):
        _load_summary(summary)

    target = tmp_path / "target.json"
    target.write_text('{"fingerprint":"x"}', encoding="utf-8")
    link = tmp_path / "link.json"
    link.symlink_to(target)
    with pytest.raises(ValueError, match="cannot open"):
        _load_summary(link)


def test_report_keeps_subscription_cost_and_claim_boundary_explicit() -> None:
    ctx = _summary(score=0.9, latency=10, rss=100, tokens=5, memory_complete=0)
    pi = _summary(score=0.8, latency=20, rss=200, tokens=10, memory_complete=10)
    points = _comparison(ctx, pi)
    result = {
        "fingerprint": "same",
        "verdict": "inconclusive",
        "point_estimates": points,
        "evidence_gates": _evidence_gates(ctx, pi, points),
    }
    report = _report(result)
    assert "point estimate is not a win claim" in report
    assert "subscription cost remains `n/a`" in report
