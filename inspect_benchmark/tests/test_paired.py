from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, patch

import pytest

from agent_benchmark.paired import (
    _evidence_status,
    _memory_evidence_complete,
    _pair_records,
    _paired_metrics,
    _report,
    _run_schedule,
    _run_warmups,
    _runtime_contracts_complete,
    _runtime_for,
    _runtime_has_no_visible_tools,
    _warmups_valid,
    main,
)


def _row(
    arm: str,
    pair: int,
    *,
    latency: float,
    tokens: int,
    exact: bool,
    runtime: str,
) -> dict[str, object]:
    return {
        "sample_id": "sample",
        "epoch": 1,
        "pair": pair,
        "order": "AB" if pair == 1 else "BA",
        "arm": arm,
        "runtime": runtime,
        "runtime_success": True,
        "exact_match": exact,
        "latency_seconds": latency,
        "usage_available": True,
        "comparable_tokens": tokens,
        "total_tokens": tokens,
        "api_equivalent_cost_usd": tokens / 1_000_000,
    }


def test_evidence_status_lists_only_failed_gates() -> None:
    gates = {
        "complete_cgroup_memory": True,
        "workspace_equivalent": True,
        "immutable_artifacts": True,
        "independent_review": False,
        "scope_is_multi_agent_team": False,
        "all_pass": False,
    }
    status = _evidence_status(gates)
    assert status == (
        "inconclusive-pending-independent_review-and-scope_is_multi_agent_team"
    )
    assert "memory" not in status
    assert "workspace" not in status
    assert _evidence_status({"all_pass": True, "ready": True}) == "complete"


def test_memory_gate_requires_every_eligible_sample() -> None:
    records = [
        {"memory_complete": True, "peak_memory_bytes": 1024},
        {"memory_complete": True, "peak_memory_bytes": 2048},
    ]
    assert _memory_evidence_complete(records)
    records[1]["memory_complete"] = False
    assert not _memory_evidence_complete(records)
    records[1]["memory_complete"] = True
    records[1].pop("peak_memory_bytes")
    assert not _memory_evidence_complete(records)


def test_ctx_contract_gate_requires_every_hashed_control() -> None:
    def identity(*, tools: bool = True) -> dict[str, object]:
        return {
            "agent_contracts": {
                "executor": {
                    "prompt": {"success": True},
                    "tools": {"success": tools},
                }
            },
            "model_contracts": {
                "provider/model": {
                    "metadata.json": {"success": True},
                    "driver": {"success": True},
                }
            },
            "unit_configurations": {
                "mount": {"complete": True},
                "agents": {"executor": {"complete": True}},
            },
        }

    assert not _runtime_contracts_complete({"agent_contracts": {}})
    assert _runtime_contracts_complete(identity())
    assert not _runtime_contracts_complete(identity(tools=False))


def test_tools_equivalence_requires_an_empty_visible_tool_set() -> None:
    empty = {
        "agent_contracts": {"executor": {"tools": {"success": True, "stdout": ""}}}
    }
    visible = {
        "agent_contracts": {
            "executor": {"tools": {"success": True, "stdout": "search\n"}}
        }
    }
    assert _runtime_has_no_visible_tools(empty)
    assert not _runtime_has_no_visible_tools(visible)


def test_compare_requires_credential_scope_attestation(monkeypatch, capsys) -> None:
    monkeypatch.setattr(
        "sys.argv",
        [
            "paired",
            "--mode",
            "compare",
            "--ctx-provider",
            "codex",
            "--pi-provider",
            "openai-codex",
            "--model",
            "model",
            "--thinking",
            "high",
        ],
    )
    with pytest.raises(SystemExit) as error:
        main()
    assert error.value.code == 2
    assert "--credential-scope-attested" in capsys.readouterr().err


def test_runtime_mapping_supports_compare_and_aa_modes() -> None:
    assert [_runtime_for("compare", arm) for arm in "ABBA"] == [
        "ctx",
        "pi",
        "pi",
        "ctx",
    ]
    assert {_runtime_for("aa-ctx", arm) for arm in "AB"} == {"ctx"}
    assert {_runtime_for("aa-pi", arm) for arm in "AB"} == {"pi"}
    with pytest.raises(ValueError, match="unknown"):
        _runtime_for("bad", "A")


def test_pair_records_preserve_ab_ba_pairs_and_positive_a_improvements() -> None:
    rows = [
        _row("A", 1, latency=1, tokens=5, exact=True, runtime="ctx"),
        _row("B", 1, latency=2, tokens=10, exact=False, runtime="pi"),
        _row("B", 2, latency=2, tokens=10, exact=False, runtime="pi"),
        _row("A", 2, latency=1, tokens=5, exact=True, runtime="ctx"),
    ]
    pairs = _pair_records(rows)
    assert len(pairs) == 2
    assert [pair["order"] for pair in pairs] == ["AB", "BA"]
    assert all(pair["quality_delta"] == 1.0 for pair in pairs)
    assert all(pair["latency_improvement"] == 0.5 for pair in pairs)
    assert all(pair["token_improvement"] == 0.5 for pair in pairs)
    metrics = _paired_metrics(pairs, "compare")
    assert metrics["latency_improvement"]["lower_95"] == 0.5
    assert metrics["latency_improvement"]["independent_samples"] == 1
    assert metrics["aa_noise_p95"] is None


def test_aa_metrics_report_ratio_noise_not_a_win() -> None:
    rows = [
        _row("A", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("B", 1, latency=1.1, tokens=11, exact=True, runtime="ctx"),
        _row("B", 2, latency=0.9, tokens=9, exact=True, runtime="ctx"),
        _row("A", 2, latency=1.0, tokens=10, exact=True, runtime="ctx"),
    ]
    metrics = _paired_metrics(_pair_records(rows), "aa-ctx")
    noise = metrics["aa_noise_p95"]
    assert isinstance(noise, dict)
    assert noise["latency_improvement"] > 0
    assert metrics["p95_is_descriptive"] is True


def test_aa_noise_keeps_signed_ratio_and_does_not_drop_ratio_two() -> None:
    rows = [
        _row("A", 1, latency=2.0, tokens=20, exact=True, runtime="ctx"),
        _row("B", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("B", 2, latency=2.0, tokens=20, exact=True, runtime="ctx"),
        _row("A", 2, latency=1.0, tokens=10, exact=True, runtime="ctx"),
    ]
    noise = _paired_metrics(_pair_records(rows), "aa-ctx")["aa_noise_p95"]
    assert isinstance(noise, dict)
    assert noise["latency_improvement"] == pytest.approx(1.0)


def test_aa_noise_uses_every_pair_not_a_per_task_geometric_average() -> None:
    rows = [
        _row("A", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("B", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("A", 2, latency=4.0, tokens=40, exact=True, runtime="ctx"),
        _row("B", 2, latency=1.0, tokens=10, exact=True, runtime="ctx"),
    ]
    noise = _paired_metrics(_pair_records(rows), "aa-ctx")["aa_noise_p95"]
    assert isinstance(noise, dict)
    assert noise["latency_improvement"] > 2.0


def test_cost_and_tokens_require_complete_usage_for_both_arms() -> None:
    rows = [
        _row("A", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("B", 1, latency=1.0, tokens=10, exact=True, runtime="pi"),
    ]
    rows[0]["usage_available"] = False
    rows[0]["api_equivalent_cost_usd"] = 1.0
    rows[1]["api_equivalent_cost_usd"] = 2.0
    pair = _pair_records(rows)[0]
    assert pair["token_improvement"] is None
    assert pair["api_cost_improvement"] is None
    assert not pair["usage_eligible"]
    assert not pair["api_cost_eligible"]


def test_failure_cannot_receive_exact_match_quality_credit() -> None:
    rows = [
        _row("A", 1, latency=1.0, tokens=10, exact=True, runtime="ctx"),
        _row("B", 1, latency=1.0, tokens=10, exact=True, runtime="pi"),
    ]
    rows[0]["runtime_success"] = False
    pair = _pair_records(rows)[0]
    assert pair["quality_delta"] == -1.0
    assert pair["success_delta"] == -1.0
    assert pair["usage_eligible"]
    assert pair["token_improvement"] == 0.0


def test_pair_records_reject_incomplete_or_duplicate_arms() -> None:
    row = _row("A", 1, latency=1, tokens=1, exact=True, runtime="ctx")
    with pytest.raises(ValueError, match="incomplete"):
        _pair_records([row])
    with pytest.raises(ValueError, match="duplicate"):
        _pair_records([row, dict(row)])


def test_warmup_touches_only_runtimes_used_by_each_mode() -> None:
    record = {
        "runtime_success": True,
        "exact_match": True,
        "usage_available": True,
        "usage_semantics": "visible-input-output-v1",
        "input_tokens": 1,
        "output_tokens": 1,
        "comparable_tokens": 2,
        "total_tokens": 2,
        "raw_frames_complete": True,
        "process_cleanup": {
            "spawned": True,
            "pgid": 123,
            "leader_start_time": 1,
            "verified_empty": True,
        },
        "cleanup_success": True,
    }
    sample = {"id": "sample", "prompt": "prompt", "target": "answer"}
    ctx_mock = AsyncMock(return_value=record)
    pi_mock = AsyncMock(return_value=record)
    with (
        patch("agent_benchmark.paired._ctx_observation", new=ctx_mock),
        patch("agent_benchmark.paired._pi_observation", new=pi_mock),
    ):
        aa_ctx = asyncio.run(
            _run_warmups(
                mode="aa-ctx",
                count=1,
                ctx_path="ctx",
                agent_name="executor",
                pi_runner=None,
                sample=sample,
                timeout=1,
                retain_frames=False,
                price_table=None,
            )
        )
        assert [row["runtime"] for row in aa_ctx] == ["ctx"]
        assert ctx_mock.await_count == 1
        assert pi_mock.await_count == 0

        ctx_mock.reset_mock()
        pi_mock.reset_mock()
        aa_pi = asyncio.run(
            _run_warmups(
                mode="aa-pi",
                count=1,
                ctx_path="ctx",
                agent_name="executor",
                pi_runner=object(),  # type: ignore[arg-type]
                sample=sample,
                timeout=1,
                retain_frames=False,
                price_table=None,
            )
        )
        assert [row["runtime"] for row in aa_pi] == ["pi"]
        assert ctx_mock.await_count == 0
        assert pi_mock.await_count == 1
        assert _warmups_valid(aa_pi, "aa-pi", 1)
        aa_pi[0]["runtime_success"] = False
        assert not _warmups_valid(aa_pi, "aa-pi", 1)
        aa_pi[0]["runtime_success"] = True
        aa_pi[0]["usage_available"] = False
        assert not _warmups_valid(aa_pi, "aa-pi", 1)


def test_schedule_executes_abba_sequentially_without_real_providers() -> None:
    ctx = {
        **_row("A", 1, latency=1, tokens=5, exact=True, runtime="ctx"),
        "input_tokens": 4,
        "output_tokens": 1,
        "completion": "answer",
        "error_code": None,
        "exit_code": 0,
    }
    pi = {
        **_row("B", 1, latency=2, tokens=10, exact=True, runtime="pi"),
        "input_tokens": 9,
        "output_tokens": 1,
        "completion": "answer",
        "error_code": None,
        "exit_code": 0,
    }
    with (
        patch(
            "agent_benchmark.paired._ctx_observation", new=AsyncMock(return_value=ctx)
        ),
        patch("agent_benchmark.paired._pi_observation", new=AsyncMock(return_value=pi)),
    ):
        records = asyncio.run(
            _run_schedule(
                mode="compare",
                ctx_path="ctx",
                agent_name="executor",
                pi_runner=object(),  # type: ignore[arg-type]
                dataset=[{"id": "sample", "prompt": "prompt", "target": "answer"}],
                repeat=1,
                timeout=1,
                retain_frames=False,
                measure_memory=False,
                price_table=None,
            )
        )
    assert [record["runtime"] for record in records] == ["ctx", "pi", "pi", "ctx"]
    assert [record["arm"] for record in records] == ["A", "B", "B", "A"]
    assert [record["pair"] for record in records] == [1, 1, 2, 2]


def test_report_never_promotes_summary_to_a_win() -> None:
    metric = {
        "observations": 2,
        "independent_samples": 1,
        "mean": 0.5,
        "lower_95": 0.5,
        "upper_95": 0.5,
    }
    summary = {
        "mode": "compare",
        "experiment": {"schedule": "ABBA"},
        "fingerprint": "workload",
        "evidence_status": "inconclusive",
        "evidence_gates": {"all_pass": False},
        "paired": {
            "quality_delta": metric,
            "success_delta": metric,
            "latency_improvement": metric,
            "token_improvement": metric,
            "api_cost_improvement": metric,
        },
    }
    report = _report(summary)
    assert "No win is claimed" in report
    assert "A/A mode estimates noise only" in report
