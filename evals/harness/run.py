#!/usr/bin/env python3
"""Run existing Rust harness contracts serially and retain auditable evidence."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import time
import uuid


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("suites.json")
TEST = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)\b")
SUMMARY = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; "
    r"(\d+) ignored; (\d+) measured; (\d+) filtered out;"
)


def load_suites():
    document = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if document.get("schema") != "cortexfs.harness-suite/v1":
        raise ValueError("unsupported harness suite schema")
    suites = document["suites"]
    ids = [suite["id"] for suite in suites]
    if not suites or len(set(ids)) != len(ids):
        raise ValueError("suite IDs must be nonempty and unique")
    for suite in suites:
        if not re.fullmatch(r"[a-z]+", suite["id"]) or not suite["required_tests"]:
            raise ValueError("each suite needs an ID and required test names")
        for fixture in suite["fixtures"]:
            if not (ROOT / fixture).is_file():
                raise ValueError(f"missing fixture: {fixture}")
    return suites


def inspect_log(path):
    counts = dict(passed=0, failed=0, ignored=0, measured=0, filtered=0)
    tests, summaries = {}, 0
    with path.open(encoding="utf-8", errors="replace") as stream:
        for line in stream:
            if match := TEST.match(line):
                tests.setdefault(match[1], []).append(match[2])
            if match := SUMMARY.match(line):
                summaries += 1
                for key, value in zip(counts, match.groups()[1:]):
                    counts[key] += int(value)
    return counts, tests, summaries


def stop_group(process):
    # The lock wrapper, Cargo, rustc and test children share this process group.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            break
        if sig == signal.SIGTERM:
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                pass
    process.wait()


def execute(command, log, timeout):
    started = time.monotonic()
    outcome, returncode, error = "completed", None, None
    environment = {**os.environ, "CARGO_TERM_COLOR": "never", "RUST_BACKTRACE": "1"}
    with log.open("x", encoding="utf-8") as stream:
        try:
            process = subprocess.Popen(
                command, cwd=ROOT, env=environment, stdin=subprocess.DEVNULL,
                stdout=stream, stderr=subprocess.STDOUT, start_new_session=True,
            )
        except OSError as failure:
            outcome, error = "error", str(failure)
        else:
            try:
                returncode = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                stop_group(process)
                outcome, returncode = "timeout", process.returncode
            except KeyboardInterrupt:
                stop_group(process)
                outcome, returncode = "interrupted", process.returncode
    counts, tests, summaries = inspect_log(log)
    passed = outcome == "completed" and returncode == 0
    passed = passed and summaries > 0 and counts["passed"] > 0 and counts["failed"] == 0
    return {
        "command": command, "outcome": outcome, "returncode": returncode,
        "error": error, "status": "passed" if passed else "failed",
        "wall_seconds": round(time.monotonic() - started, 3),
        "counts": counts, "summary_count": summaries, "tests": tests,
        "log": log.name,
    }


def assess(suite, invocation):
    checks = []
    for name in suite["required_tests"]:
        observed = invocation["tests"].get(name, []) if invocation else []
        checks.append({"name": name, "passed": bool(observed) and all(
            status == "ok" for status in observed
        ), "observed": observed})
    passed = invocation and invocation["status"] == "passed" and all(
        check["passed"] for check in checks
    )
    return {"id": suite["id"], "title": suite["title"], "checks": checks,
            "status": "passed" if passed else "failed" if invocation else "not_run"}


def identify(command):
    try:
        return subprocess.check_output(
            command, cwd=ROOT, stderr=subprocess.DEVNULL, text=True, timeout=10
        ).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def write_report(output, report):
    (output / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    lines = ["# CortexFS harness evaluation", "", f"Result: **{report['status']}**",
             f"Source: `{report['source']['commit']}`; profile: `{report['profile']}`.",
             "", "Deterministic contract evidence; no model-quality or performance score.",
             "Wall time includes Cargo compilation and lock wait.", "",
             "| Contract | Status | Required tests passed |", "| --- | --- | --- |"]
    for suite in report["suites"]:
        passed = sum(check["passed"] for check in suite["checks"])
        lines.append(f"| {suite['title']} | {suite['status']} | {passed}/{len(suite['checks'])} |")
    lines.extend(["", "## Invocations", ""])
    for invocation in report["invocations"]:
        counts = invocation["counts"]
        lines.append(
            f"- [{invocation['id']}]({invocation['log']}): {invocation['status']} "
            f"({invocation['outcome']}, exit {invocation['returncode']}); "
            f"{counts['passed']} passed, {counts['failed']} failed, "
            f"{counts['ignored']} ignored; {invocation['wall_seconds']} s."
        )
    missing = [check["name"] for suite in report["suites"] for check in suite["checks"]
               if not check["passed"]]
    if missing:
        lines.extend(["", "## Required tests without passing evidence", ""])
        lines.extend(f"- `{name}`" for name in missing)
    lines.extend(["", "## Scope limits", ""])
    lines.extend(f"- {limit}" for limit in report["limits"])
    (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="list contracts without running Cargo")
    parser.add_argument("--suite", action="append", help="run a selected contract (repeatable)")
    parser.add_argument("--workspace", action="store_true", help="evaluate the full workspace CI gate")
    parser.add_argument("--offline", action="store_true", help="require an already cached Cargo dependency set")
    parser.add_argument("--timeout", type=float, default=3600, help="per-Cargo wall timeout, including build and lock wait")
    parser.add_argument("--output", type=Path, help="new output directory; existing directories are refused")
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and positive")
    if args.workspace and args.suite:
        parser.error("--workspace and --suite are mutually exclusive")
    try:
        suites = load_suites()
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))
    known = {suite["id"] for suite in suites}
    if args.suite and set(args.suite) - known:
        parser.error("unknown suite: " + ", ".join(sorted(set(args.suite) - known)))
    suites = [suite for suite in suites if not args.suite or suite["id"] in args.suite]
    if args.list:
        for suite in suites:
            print(f"{suite['id']}: {suite['title']} ({len(suite['required_tests'])} required tests)")
        return 0
    if platform.system() != "Linux":
        parser.error("the harness requires Linux Unix sockets and filesystem semantics")
    started = datetime.now(timezone.utc).isoformat()
    output = args.output or ROOT / "target/harness-eval" / (started.replace(":", "-") + "-" + uuid.uuid4().hex[:8])
    output = output.resolve()
    try:
        output.mkdir(parents=True, exist_ok=False, mode=0o700)
    except OSError as error:
        parser.error(f"cannot create a fresh output directory: {error}")
    # CI deliberately runs tests as a different UID from the checkout owner.
    git = ["git", "-c", f"safe.directory={ROOT}"]
    changes = identify([*git, "status", "--porcelain", "--untracked-files=normal"])
    source = {"commit": identify([*git, "rev-parse", "HEAD"]),
              "dirty": bool(changes) if changes is not None else None,
              "manifest_sha256": hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
              "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    report = {"schema": "cortexfs.harness-evaluation/v1", "started_at": started,
              "profile": "workspace" if args.workspace else "selected" if args.suite else "contracts",
              "source": source, "environment": {"platform": platform.platform(),
              "python": platform.python_version(), "rustc": identify(["rustc", "--version"]),
              "cargo": identify(["cargo", "--version"]), "offline": args.offline,
              "timeout_seconds": args.timeout}, "invocations": [], "suites": [],
              "limits": ["No paid API, configured provider or model inference is invoked.",
              "No general agent task-success, token/cost, TTFT, p95 or RSS claims.",
              "Typed v2 frames do not verify a persistent concurrent v2 runtime.",
              "Focused contracts do not prove mounted FUSE, systemd/cgroup or kernel sandbox integration.",
              "Existing platform-conditional test branches may return early; inspect raw logs and fixture sources."]}
    plans = [{"id": "workspace", "cargo_args": ["--workspace", "--all-targets", "--all-features"]}] if args.workspace else suites
    invocations = {}
    for plan in plans:
        command = [str(ROOT / "scripts/serialize-cargo.sh"), "cargo", "test", "--locked"]
        if args.offline:
            command.append("--offline")
        command.extend(plan["cargo_args"])
        command.extend(["--", "--test-threads=1", "--format=pretty", "--color=never"])
        print(f"Running {plan['id']}; log: {output / (plan['id'] + '.log')}", flush=True)
        invocation = execute(command, output / (plan["id"] + ".log"), args.timeout)
        invocation["id"] = plan["id"]
        invocations[plan["id"]] = invocation
        report["invocations"].append(invocation)
        report["suites"] = [assess(suite, invocations.get("workspace" if args.workspace else suite["id"])) for suite in suites]
        report["status"] = "passed" if all(suite["status"] == "passed" for suite in report["suites"]) else "failed"
        write_report(output, report)
        print(f"{plan['id']}: {invocation['status']} ({invocation['counts']['passed']} tests passed)", flush=True)
        if invocation["status"] != "passed":
            break
    print(f"{report['status']}: {output / 'report.md'}", flush=True)
    if any(item["outcome"] == "interrupted" for item in report["invocations"]):
        return 130
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
