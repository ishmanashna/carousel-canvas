"""Thread-safe span aggregation; writes Markdown + JSON when requested."""

from __future__ import annotations

import atexit
import json
import os
import threading
import time
from collections import deque
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterator

_MAX_SAMPLES = 800

_enabled = os.environ.get("CAROUSEL_CANVAS_PERF", "").lower() in ("1", "true", "yes")
_out_dir: Path | None = None
_lock = threading.Lock()
_rows: dict[str, dict[str, Any]] = {}


def is_enabled() -> bool:
    return _enabled


def enable(*, out_dir: Path | None = None) -> None:
    """Turn tracing on (used by the automated harness before importing the main window)."""
    global _enabled, _out_dir
    _enabled = True
    if out_dir is not None:
        _out_dir = out_dir


def set_out_dir(path: Path | None) -> None:
    global _out_dir
    _out_dir = path


def reset() -> None:
    with _lock:
        _rows.clear()


def _row(name: str) -> dict[str, Any]:
    r = _rows.get(name)
    if r is None:
        r = {
            "count": 0,
            "total_s": 0.0,
            "min_s": float("inf"),
            "max_s": 0.0,
            "samples": deque(maxlen=_MAX_SAMPLES),
        }
        _rows[name] = r
    return r


def _record(name: str, dt_s: float) -> None:
    if not _enabled or dt_s < 0:
        return
    with _lock:
        r = _row(name)
        r["count"] += 1
        r["total_s"] += dt_s
        r["min_s"] = min(r["min_s"], dt_s)
        r["max_s"] = max(r["max_s"], dt_s)
        r["samples"].append(dt_s)


def record_since(name: str, t0: float) -> None:
    _record(name, time.perf_counter() - t0)


@contextmanager
def span(name: str) -> Iterator[None]:
    if not _enabled:
        yield
        return
    t0 = time.perf_counter()
    try:
        yield
    finally:
        _record(name, time.perf_counter() - t0)


def _percentile(sorted_vals: list[float], p: float) -> float:
    if not sorted_vals:
        return 0.0
    i = min(int(round((len(sorted_vals) - 1) * p)), len(sorted_vals) - 1)
    return sorted_vals[i]


def _default_out_dir() -> Path:
    if _out_dir is not None:
        return _out_dir
    return Path.cwd() / "perf_reports"


def write_report(*, label: str = "gui") -> Path:
    """Write perf_reports/perf_<label>_<utc>.md and .json; return Markdown path."""
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    out = _default_out_dir()
    out.mkdir(parents=True, exist_ok=True)
    base = out / f"perf_{label}_{stamp}"

    with _lock:
        snapshot = {k: dict(v) for k, v in _rows.items()}
        for v in snapshot.values():
            v["samples"] = list(v["samples"])

    table: list[dict[str, Any]] = []
    for name, r in sorted(snapshot.items(), key=lambda kv: (-kv[1]["total_s"], kv[0])):
        n = int(r["count"])
        if n == 0:
            continue
        total = float(r["total_s"])
        mean_ms = (total / n) * 1000.0
        smin = float(r["min_s"]) * 1000.0 if r["min_s"] != float("inf") else 0.0
        smax = float(r["max_s"]) * 1000.0
        samples = sorted(float(x) for x in r["samples"])
        p50 = _percentile(samples, 0.50) * 1000.0
        p95 = _percentile(samples, 0.95) * 1000.0
        table.append(
            {
                "span": name,
                "count": n,
                "total_ms": round(total * 1000.0, 2),
                "mean_ms": round(mean_ms, 3),
                "min_ms": round(smin, 3),
                "max_ms": round(smax, 3),
                "p50_ms": round(p50, 3),
                "p95_ms": round(p95, 3),
            }
        )

    md_lines = [
        f"# Carousel Canvas GUI — performance report (`{label}`)",
        "",
        f"- UTC: `{stamp}`",
        f"- Spans are wall time; background work appears under `gui.preview.*` and similar.",
        "",
        "| span | count | total (ms) | mean (ms) | min (ms) | p50 (ms) | p95 (ms) | max (ms) |",
        "|------|------:|-----------:|----------:|---------:|---------:|---------:|---------:|",
    ]
    for row in table:
        md_lines.append(
            f"| `{row['span']}` | {row['count']} | {row['total_ms']:.2f} | {row['mean_ms']:.3f} | "
            f"{row['min_ms']:.3f} | {row['p50_ms']:.3f} | {row['p95_ms']:.3f} | {row['max_ms']:.3f} |"
        )
    md_lines.append("")
    md_path = base.with_suffix(".md")
    md_path.write_text("\n".join(md_lines), encoding="utf-8")

    json_path = base.with_suffix(".json")
    json_path.write_text(json.dumps({"label": label, "spans": table}, indent=2), encoding="utf-8")
    return md_path


def _atexit_dump() -> None:
    if not _enabled or not _rows:
        return
    try:
        p = write_report(label="atexit")
        print(f"[CAROUSEL_CANVAS_PERF] Wrote {p}", flush=True)
    except Exception:
        pass


def register_atexit_report() -> None:
    if _enabled:
        atexit.register(_atexit_dump)
