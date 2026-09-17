"""One GPU job at a time. Stale locks (crashed agents) are stolen after 20 minutes."""

from __future__ import annotations

import os
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from cache import cache_root

STALE_SECONDS = 20 * 60


def gpu_lock_path() -> Path:
    return cache_root() / ".gpu.lock"


@contextmanager
def gpu_lock() -> Iterator[None]:
    path = gpu_lock_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    while True:
        try:
            fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_RDWR)
            os.write(fd, str(os.getpid()).encode("ascii"))
            break
        except FileExistsError:
            try:
                age = time.time() - path.stat().st_mtime
            except OSError:
                age = STALE_SECONDS + 1
            if age > STALE_SECONDS:
                path.unlink(missing_ok=True)
                continue
            time.sleep(2)
    try:
        yield
    finally:
        os.close(fd)
        path.unlink(missing_ok=True)
