"""Shared image I/O: paths, colors, JPEG save (copied per product after repo split)."""

from __future__ import annotations

import threading
from io import BytesIO
from pathlib import Path
from typing import Tuple

from PIL import Image, ImageColor, ImageOps

# User-facing JPEG deliverables (stacker collages + carousel slices): must be ≤ this size.
MAX_OUTPUT_JPEG_BYTES = 8 * 1024 * 1024  # 8MB (Instagram upload limit)
MAX_FILE_SIZE = MAX_OUTPUT_JPEG_BYTES
CAROUSEL_SLICE_MAX_BYTES = MAX_OUTPUT_JPEG_BYTES

IMAGE_EXTENSIONS = frozenset({".jpg", ".jpeg", ".png"})


def parse_color(color_str: str | Tuple[int, int, int]) -> Tuple[int, int, int]:
    """Convert color name or hex to RGB tuple. Falls back to white on error."""
    if not isinstance(color_str, str):
        return color_str  # type: ignore[return-value]
    try:
        s = str(color_str).strip()
        if s.startswith("#"):
            h = s.lstrip("#")
            if len(h) == 6 and all(c in "0123456789AaBbCcDdEeFf" for c in h):
                return tuple(int(h[i : i + 2], 16) for i in (0, 2, 4))  # type: ignore[return-value]
        return ImageColor.getrgb(s)
    except Exception:
        return (255, 255, 255)


def fix_orientation(img: Image.Image) -> Image.Image:
    try:
        return ImageOps.exif_transpose(img)
    except Exception:
        return img


def flatten_alpha(img: Image.Image) -> Image.Image:
    if img.mode in ("RGBA", "LA", "P"):
        background = Image.new("RGB", img.size, (255, 255, 255))
        if img.mode == "P":
            img = img.convert("RGBA")
        mask = img.split()[-1] if img.mode in ("RGBA", "LA") else None
        background.paste(img, mask=mask)
        return background
    if img.mode != "RGB":
        return img.convert("RGB")
    return img


def save_optimized(
    image: Image.Image,
    output_path: Path | str,
    *,
    max_file_size: int | None = MAX_FILE_SIZE,
    jpeg_quality_first: int | None = None,
    jpeg_quality_min: int | None = None,
    jpeg_quality_max: int | None = None,
    jpeg_optimize: bool = False,
) -> None:
    output_path = Path(output_path)
    save_kw: dict[str, object] = {"subsampling": 0}
    if jpeg_optimize:
        save_kw["optimize"] = True

    def _jpeg_to_buf(q: int) -> BytesIO:
        b = BytesIO()
        image.save(b, "JPEG", quality=q, **save_kw)
        return b

    use_hq_slice = (
        jpeg_quality_first is not None
        and jpeg_quality_min is not None
        and jpeg_quality_max is not None
    )
    if use_hq_slice:
        q_uncapped = min(int(jpeg_quality_first), int(jpeg_quality_max))
    else:
        q_uncapped = int(jpeg_quality_first) if jpeg_quality_first is not None else 95
    buf = _jpeg_to_buf(q_uncapped)
    if max_file_size is None:
        output_path.write_bytes(buf.getvalue())
        return

    if buf.tell() <= max_file_size:
        output_path.write_bytes(buf.getvalue())
        return

    if use_hq_slice:
        lo, hi = int(jpeg_quality_min), int(q_uncapped) - 1
        best_buf = None
        while lo <= hi:
            mid = (lo + hi) // 2
            b = _jpeg_to_buf(mid)
            if b.tell() <= max_file_size:
                best_buf = b
                lo = mid + 1
            else:
                hi = mid - 1
        if best_buf is None:
            best_buf = _jpeg_to_buf(int(jpeg_quality_min))
        output_path.write_bytes(best_buf.getvalue())
        return

    lo, hi = 65, 90
    best_buf = None
    while lo <= hi:
        mid = (lo + hi) // 2
        b = _jpeg_to_buf(mid)
        if b.tell() <= max_file_size:
            best_buf = b
            lo = mid + 1
        else:
            hi = mid - 1
    if best_buf is None:
        best_buf = _jpeg_to_buf(65)
    output_path.write_bytes(best_buf.getvalue())


_gp_lock = threading.Lock()
_gp_folder_sig: dict[str, tuple[int, int]] = {}
_gp_cache: dict[tuple[str, str], list[Path]] = {}


def get_image_aspect_hint(image_path: Path) -> str:
    """After EXIF orientation: ``landscape``, ``portrait``, or ``square`` (or on read error)."""
    try:
        with Image.open(image_path) as img:
            exif = img.getexif()
            orientation = exif.get(0x0112, 1) if exif else 1
            w, h = img.size
            if orientation in (5, 6, 7, 8):
                w, h = h, w
            if w > h:
                return "landscape"
            if h > w:
                return "portrait"
            return "square"
    except Exception:
        return "square"


def _check_orientation_fast(image_path: Path, required_orientation: str) -> bool:
    if required_orientation == "mixed":
        try:
            with Image.open(image_path) as img:
                img.load()
            return True
        except Exception:
            return False
    try:
        with Image.open(image_path) as img:
            exif = img.getexif()
            orientation = exif.get(0x0112, 1) if exif else 1
            w, h = img.size
            if orientation in (5, 6, 7, 8):
                w, h = h, w
            return w > h if required_orientation == "horizontal" else h > w
    except Exception:
        return False


def get_valid_paths(folder_path: str | Path, required_orientation: str) -> list[Path]:
    from app.perf.tracker import span

    folder = Path(folder_path).resolve()
    if not folder.is_dir():
        return []
    key_f = str(folder)
    kt = (key_f, required_orientation)
    try:
        st = folder.stat()
        sig = (int(getattr(st, "st_mtime_ns", int(st.st_mtime * 1e9))), int(getattr(st, "st_ino", 0)))
    except OSError:
        return []

    with _gp_lock:
        if _gp_folder_sig.get(key_f) != sig:
            for k in list(_gp_cache.keys()):
                if k[0] == key_f:
                    del _gp_cache[k]
            _gp_folder_sig[key_f] = sig
        hit = _gp_cache.get(kt)
        if hit is not None:
            return list(hit)

    with span("engine.get_valid_paths"):
        valid = []
        for p in folder.iterdir():
            if p.is_file() and p.suffix.lower() in IMAGE_EXTENSIONS and _check_orientation_fast(
                p, required_orientation
            ):
                valid.append(p)
        out = sorted(valid)

    with _gp_lock:
        if _gp_folder_sig.get(key_f) == sig:
            _gp_cache[kt] = out
    return list(out)
