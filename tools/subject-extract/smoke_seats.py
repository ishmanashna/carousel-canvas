"""Smoke: two waiters serialize on gpu_lock; wipe-islands drops a floating speck."""
from __future__ import annotations

import sys
import threading
import time
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

from gpu_lock import gpu_lock, gpu_lock_path  # noqa: E402
from compose_cmd import run_wipe_islands  # noqa: E402


def hold(sec: float, label: str, events: list) -> None:
    t_wait = time.perf_counter()
    with gpu_lock():
        waited = time.perf_counter() - t_wait
        events.append((label, "acquired", waited))
        time.sleep(sec)
        events.append((label, "released", time.perf_counter()))


def test_lock() -> None:
    events: list = []
    a = threading.Thread(target=hold, args=(1.2, "A", events))
    b = threading.Thread(target=hold, args=(0.4, "B", events))
    t0 = time.perf_counter()
    a.start()
    time.sleep(0.15)
    b.start()
    a.join()
    b.join()
    elapsed = time.perf_counter() - t0
    assert elapsed >= 1.5, elapsed
    assert not gpu_lock_path().exists()
    waits = {label: waited for label, kind, waited in events if kind == "acquired"}
    assert waits["A"] < 0.3, waits
    assert waits["B"] >= 0.9, waits
    print("seat_lock_ok", round(elapsed, 2), "B_wait", round(waits["B"], 2))


def test_wipe_islands() -> None:
    out = ROOT / "_smoke_wipe.png"
    arr = np.zeros((200, 200, 4), dtype=np.uint8)
    arr[40:160, 40:120, :3] = 200
    arr[40:160, 40:120, 3] = 255  # person-sized
    arr[10:18, 170:178, :3] = 255
    arr[10:18, 170:178, 3] = 255  # floating speck
    Image.fromarray(arr, "RGBA").save(out)
    code = run_wipe_islands(str(out), str(out))
    assert code == 0, code
    got = np.array(Image.open(out))
    assert got[100, 80, 3] > 200
    assert got[14, 174, 3] == 0
    out.unlink(missing_ok=True)
    print("wipe_islands_ok")


if __name__ == "__main__":
    test_lock()
    test_wipe_islands()
    print("smoke_ok")
