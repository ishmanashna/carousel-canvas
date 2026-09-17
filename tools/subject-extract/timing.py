"""Per-command elapsed logging for the subject-extract CLI."""

from __future__ import annotations

import json
import os
import time
from datetime import datetime
from pathlib import Path

PHASES: dict[str, float | int | str] = {}


def note(key: str, value: float | int | str) -> None:
    PHASES[key] = value


def clear() -> None:
    PHASES.clear()


def snapshot() -> dict[str, float | int | str]:
    return dict(PHASES)


class Span:
    def __init__(self) -> None:
        self._t0 = time.perf_counter()

    def seconds(self) -> float:
        return time.perf_counter() - self._t0


def log_command(
    command: str,
    elapsed_seconds: float,
    exit_code: int,
    argv: list[str],
) -> None:
    extra = snapshot()
    clear()
    rec: dict[str, object] = {
        "ts": datetime.now().astimezone().isoformat(timespec="seconds"),
        "command": command,
        "elapsed_seconds": round(elapsed_seconds, 3),
        "exit": exit_code,
        "argv": argv[:24],
    }
    rec.update(extra)
    parts = [
        f"sx-time command={command}",
        f"elapsed_seconds={elapsed_seconds:.3f}",
        f"exit={exit_code}",
    ]
    for key, value in extra.items():
        parts.append(f"{key}={value}")
    print(" ".join(parts), flush=True)
    log_path = os.environ.get("SX_ACTS_LOG", "").strip()
    if not log_path:
        return
    path = Path(log_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(rec, ensure_ascii=True) + "\n")
