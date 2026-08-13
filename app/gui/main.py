"""GUI entry point. Use ``if __name__ == '__main__'`` for PyInstaller and multiprocessing."""

from __future__ import annotations

import logging
import multiprocessing
import os
import sys
from pathlib import Path


def _parse_perf_cli() -> tuple[bool, Path | None]:
    """Strip ``--perf`` / ``--perf-out DIR`` from sys.argv; return (want_perf, out_dir or None)."""
    want = os.environ.get("CAROUSEL_CANVAS_PERF", "").lower() in ("1", "true", "yes")
    out: Path | None = None
    keep: list[str] = [sys.argv[0]]
    i = 1
    while i < len(sys.argv):
        a = sys.argv[i]
        if a == "--perf":
            want = True
            i += 1
            continue
        if a == "--perf-out" and i + 1 < len(sys.argv):
            out = Path(sys.argv[i + 1])
            want = True
            i += 2
            continue
        keep.append(a)
        i += 1
    sys.argv[:] = keep
    return want, out


def _configure_logging() -> None:
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(message)s", "%Y-%m-%d %H:%M:%S")
    base = os.environ.get("LOCALAPPDATA") or str(Path.home())
    log_dir = Path(base) / "CarouselCanvas" / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    fh = logging.FileHandler(log_dir / "app.log", encoding="utf-8")
    fh.setFormatter(fmt)
    root.addHandler(fh)
    if sys.stdout and getattr(sys.stdout, "isatty", lambda: False)():
        sh = logging.StreamHandler(sys.stdout)
        sh.setFormatter(fmt)
        root.addHandler(sh)


def main() -> None:
    want_perf, perf_out = _parse_perf_cli()
    if want_perf:
        os.environ["CAROUSEL_CANVAS_PERF"] = "1"

    _configure_logging()
    import tkinter as tk

    if os.environ.get("CAROUSEL_CANVAS_PERF", "").lower() in ("1", "true", "yes"):
        from app.perf.tracker import enable, register_atexit_report, set_out_dir

        enable()
        if perf_out is not None:
            set_out_dir(perf_out)
        register_atexit_report()

    from app.gui.main_window import MainWindow

    root = tk.Tk()
    MainWindow(root)
    root.mainloop()


if __name__ == "__main__":
    multiprocessing.freeze_support()
    main()
