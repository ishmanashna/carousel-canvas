"""One GPU job at a time.

File lock under the subject-extract cache. Holder writes pid + heartbeat.
Waiters block with backoff. Stale locks are stolen when the holder PID is dead
or the heartbeat is older than STALE_SECONDS (crashed agent / killed overnight).
"""

from __future__ import annotations

import ctypes
import os
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from cache import cache_root
from timing import note

STALE_SECONDS = 20 * 60
HEARTBEAT_SECONDS = 15
POLL_SECONDS = 2.0
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000


def gpu_lock_path() -> Path:
    return cache_root() / ".gpu.lock"


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        try:
            handle = ctypes.windll.kernel32.OpenProcess(  # type: ignore[attr-defined]
                PROCESS_QUERY_LIMITED_INFORMATION, False, pid
            )
            if handle:
                ctypes.windll.kernel32.CloseHandle(handle)  # type: ignore[attr-defined]
                return True
            err = ctypes.windll.kernel32.GetLastError()  # type: ignore[attr-defined]
            return err == 5
        except Exception:  # noqa: BLE001
            return True
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def _read_holder(path: Path) -> tuple[int | None, float]:
    try:
        text = path.read_text(encoding="ascii", errors="ignore").strip()
        pid = int(text.split()[0]) if text else None
    except (OSError, ValueError):
        pid = None
    try:
        mtime = path.stat().st_mtime
    except OSError:
        mtime = 0.0
    return pid, mtime


def _write_holder(fd: int, pid: int) -> None:
    os.lseek(fd, 0, os.SEEK_SET)
    payload = f"{pid} {time.time():.3f}".encode("ascii")
    os.write(fd, payload)
    try:
        os.ftruncate(fd, len(payload))
    except OSError:
        pass


@contextmanager
def gpu_lock() -> Iterator[None]:
    path = gpu_lock_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    t0 = time.perf_counter()
    fd: int | None = None
    stop_hb = threading.Event()
    hb_thread: threading.Thread | None = None

    while True:
        try:
            fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_RDWR)
            _write_holder(fd, os.getpid())
            break
        except FileExistsError:
            pid, mtime = _read_holder(path)
            age = time.time() - mtime if mtime else STALE_SECONDS + 1
            dead = pid is not None and not _pid_alive(pid)
            if dead or age > STALE_SECONDS:
                try:
                    path.unlink(missing_ok=True)
                    note(
                        "gpu_lock_steal",
                        f"dead={int(bool(dead))} age={age:.1f} pid={pid}",
                    )
                except OSError:
                    pass
                continue
            time.sleep(POLL_SECONDS)

    waited = time.perf_counter() - t0
    note("gpu_lock_wait_seconds", round(waited, 3))

    def _heartbeat() -> None:
        while not stop_hb.wait(HEARTBEAT_SECONDS):
            try:
                if fd is None:
                    break
                _write_holder(fd, os.getpid())
                path.touch()
            except OSError:
                break

    hb_thread = threading.Thread(target=_heartbeat, name="gpu-lock-hb", daemon=True)
    hb_thread.start()
    try:
        yield
    finally:
        stop_hb.set()
        if hb_thread is not None:
            hb_thread.join(timeout=1.0)
        if fd is not None:
            try:
                os.close(fd)
            except OSError:
                pass
        try:
            path.unlink(missing_ok=True)
        except OSError:
            pass
