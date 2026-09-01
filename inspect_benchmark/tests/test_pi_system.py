# pyright: reportMissingImports=false
from __future__ import annotations

import argparse
import asyncio
import stat
import tempfile
import textwrap
from pathlib import Path

import pytest

from agent_benchmark.pi import PiJsonlRunner, PiUsage
from agent_benchmark.pi_system import (
    ApiPriceTable,
    _price_table,
    _report,
    _samples,
)
from agent_benchmark.system import _aggregate


def _executable(root: Path) -> Path:
    path = root / "pi-fixture"
    path.write_text(
        textwrap.dedent(
            """\
            #!/usr/bin/env python3
            import json

            frames = [
                {
                    "type": "message_update",
                    "assistantMessageEvent": {"type": "text_delta", "delta": "4"},
                },
                {
                    "type": "message_end",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "4"}],
                        "provider": "provider",
                        "model": "model",
                        "stopReason": "stop",
                        "usage": {
                            "input": 10,
                            "output": 2,
                            "cacheRead": 3,
                            "cacheWrite": 4,
                            "totalTokens": 19,
                        },
                    },
                },
                {"type": "agent_settled"},
            ]
            for frame in frames:
                print(json.dumps(frame), flush=True)
            """
        ),
        encoding="utf-8",
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def test_price_table_requires_versioned_source_and_complete_rates() -> None:
    empty = argparse.Namespace(
        input_usd_per_million=None,
        output_usd_per_million=None,
        cache_read_usd_per_million=None,
        cache_write_usd_per_million=None,
        price_source=None,
    )
    assert _price_table(empty) is None

    missing_source = argparse.Namespace(
        input_usd_per_million=1.0,
        output_usd_per_million=2.0,
        cache_read_usd_per_million=None,
        cache_write_usd_per_million=None,
        price_source=None,
    )
    with pytest.raises(ValueError, match="price-source"):
        _price_table(missing_source)


def test_equivalent_cost_is_explicitly_token_weighted() -> None:
    prices = ApiPriceTable("fixture-2026-01-01", 1.0, 2.0, 0.5, 3.0)
    usage = PiUsage(
        input_tokens=1_000_000,
        output_tokens=500_000,
        cache_read_tokens=2_000_000,
        cache_write_tokens=100_000,
    )
    assert prices.equivalent_cost(usage) == pytest.approx(3.3)


def test_samples_flatten_pi_evidence_for_shared_aggregate() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        runner = PiJsonlRunner(
            pi_path=str(_executable(root)),
            provider="provider",
            model="model",
            thinking="medium",
            timeout=1,
            workspace=str(root),
        )
        records = asyncio.run(
            _samples(
                runner=runner,
                dataset=[{"id": "math", "prompt": "2+2", "target": "4"}],
                repeat=1,
                price_table=ApiPriceTable("fixture", 1.0, 2.0),
            )
        )

    assert len(records) == 1
    assert records[0]["exact_match"] is True
    assert records[0]["total_tokens"] == 19
    assert records[0]["api_equivalent_cost_usd"] == pytest.approx(0.000014)
    metrics = _aggregate(records)
    assert metrics["runtime_success_rate"] == 1.0
    assert metrics["rss_samples"] == 1
    assert metrics["attributable_subscription_cost_usd"] is None


def test_report_carries_inconclusive_evidence_label() -> None:
    record = {
        "runtime_success": True,
        "latency_seconds": 1.0,
        "ttft_seconds": 0.5,
        "input_tokens": 1,
        "output_tokens": 1,
        "total_tokens": 2,
        "completion": "4",
        "exact_match": True,
        "error_code": None,
        "exit_code": 0,
        "peak_rss_bytes": 1024,
        "usage_available": True,
    }
    summary = {
        "run_id": "run",
        "fingerprint": "workload",
        "treatment_fingerprint": "treatment",
        "overall": _aggregate([record]),
        "evidence_status": "inconclusive: fixture",
        "artifacts": {
            "raw_frames_complete_samples": 0,
            "samples_jsonl_sha256": "fixture",
        },
    }
    report = _report(summary)
    assert "inconclusive: fixture" in report
    assert "Attributable subscription cost: `n/a`" in report
