from pathlib import Path
from typing import Any

from inspect_ai import Task, task
from inspect_ai.dataset import Sample, json_dataset
from inspect_ai.scorer import accuracy, exact, grouped, stderr
from inspect_ai.solver import generate


def _dataset_path() -> str:
    return str(Path(__file__).with_name("data") / "agent_benchmark.jsonl")


def record_to_sample(record: dict[str, Any]) -> Sample:
    return Sample(
        input=record["prompt"],
        target=record["target"],
        id=record.get("id"),
        metadata={
            "category": record.get("category", "general"),
            "difficulty": record.get("difficulty", "easy"),
            "source": record.get("source", "local"),
        },
    )


def _metrics() -> list[Any]:
    return [
        accuracy(),
        stderr(),
        grouped(
            accuracy(),
            "category",
            all=False,
            name_template="category_{group_name}",
        ),
        grouped(
            accuracy(),
            "difficulty",
            all=False,
            name_template="difficulty_{group_name}",
        ),
    ]


@task
def agent_benchmark(dataset: str = _dataset_path()) -> Task:
    """Evaluate deterministic quality tasks with category-level metrics."""

    return Task(
        dataset=json_dataset(dataset, record_to_sample),
        scorer=exact(),
        solver=generate(),
        metrics=_metrics(),
        metadata={"suite": "agent_benchmark", "task": "agent_benchmark"},
    )


@task
def agent_benchmark_json_mode(dataset: str = _dataset_path()) -> Task:
    """Run the same dataset while marking structured-output expectations."""

    return Task(
        dataset=json_dataset(dataset, record_to_sample),
        scorer=exact(),
        solver=generate(),
        metrics=_metrics(),
        metadata={
            "suite": "agent_benchmark",
            "task": "agent_benchmark_json_mode",
            "answer_format": "json",
        },
    )
