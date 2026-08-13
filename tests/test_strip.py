"""Strip carousel slicer + template geometry (run: python -m unittest tests.test_strip -v)."""

from __future__ import annotations

import random
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from app.image_ops import process_image_contained_panned
from app.io import MAX_OUTPUT_JPEG_BYTES, get_valid_paths
from app.strip.assign import expand_paths_cyclic, pick_smart_fills
from app.strip.slicer import slice_wide_to_carousel
from app.strip.composer import compose_strip_wide, resolve_strip_slots_for_preview
from app.strip.template_data import (
    STRIP_GUI_TEMPLATE_IDS,
    STRIP_SLICE_COUNT,
    StripSlotDef,
    StripTemplate,
    build_seamless_mosaic_v1_slots,
    get_default_template,
    get_template_by_id,
    mosaic_v1_max_slot_count,
    strip_background_underfill_count,
    strip_effective_num_slots,
    strip_image_slot_count,
)


def _project_root() -> Path:
    return Path(__file__).resolve().parent.parent


def _strip_smoke_source_paths(tmp_path: Path, need: int) -> list[Path]:
    """
    Prefer repo ``TEST IMAGES`` (or first immediate subfolder with enough files).
    Otherwise synthesize JPGs under ``tmp_path``.
    """
    marker = _project_root() / "TEST IMAGES"
    if marker.is_dir():
        paths = get_valid_paths(marker, "mixed")
        if len(paths) >= need:
            return list(paths)
        for sub in sorted(marker.iterdir()):
            if sub.is_dir():
                paths = get_valid_paths(sub, "mixed")
                if len(paths) >= need:
                    return list(paths)
    tmp_path.mkdir(parents=True, exist_ok=True)
    for i in range(need):
        Image.new("RGB", (220, 280 + (i % 40)), color=((i * 17) % 256, 50, 80)).save(
            tmp_path / f"p{i:03d}.jpg"
        )
    return sorted(tmp_path.glob("*.jpg"))


class TestStripSlicer(unittest.TestCase):
    def test_slice_non_overlapping(self) -> None:
        tpl = get_default_template()
        wide = Image.new("RGB", (tpl.canvas_width, tpl.canvas_height), color=(128, 64, 32))
        slices = slice_wide_to_carousel(
            wide, tpl.slice_width, tpl.slice_height, tpl.slice_count, tpl.overlap_px
        )
        self.assertEqual(len(slices), 10)
        for sl in slices:
            self.assertEqual(sl.size, (1080, 1350))

    def test_slice_overlap(self) -> None:
        w, h, sw, sh, ov = 2000, 400, 500, 400, 100
        wide = Image.new("RGB", (w, h), color="white")
        slices = slice_wide_to_carousel(wide, sw, sh, 4, ov)
        self.assertEqual(len(slices), 4)
        stride = sw - ov
        self.assertLessEqual(3 * stride + sw, w)

    def test_slice_too_narrow_raises(self) -> None:
        wide = Image.new("RGB", (500, 1350), color="black")
        with self.assertRaises(ValueError):
            slice_wide_to_carousel(wide, 1080, 1350, 10, 0)


class TestStripSmartAssign(unittest.TestCase):
    def test_prefer_landscape_nearly_square_slot_gets_landscape_file(self) -> None:
        """Wide slots below w/h≥1.2 must still request landscape when flagged (mosaic col 2)."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            land = tmp_path / "land.jpg"
            port = tmp_path / "port.jpg"
            Image.new("RGB", (500, 200), color=(255, 0, 0)).save(land)
            Image.new("RGB", (200, 500), color=(0, 0, 255)).save(port)
            slots = (
                StripSlotDef(0, 0, 800, 1200, z_index=0, prefer_portrait=True),
                StripSlotDef(
                    0, 0, 1400, 1350, z_index=1, prefer_landscape=True, cover_height_first=True
                ),
            )
            tpl = StripTemplate(
                id="test_land_pref",
                canvas_width=2400,
                canvas_height=1350,
                slice_width=1080,
                slice_height=1350,
                slice_count=2,
                overlap_px=0,
                background="white",
                slots=slots,
            )
            rng = random.Random(99)
            chosen = pick_smart_fills([land, port], tpl, rng=rng)
            self.assertIs(chosen[0], port)
            self.assertIs(chosen[1], land)

    def test_smart_prefers_orientation_to_slot_aspect(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            land = tmp_path / "land.jpg"
            port = tmp_path / "port.jpg"
            Image.new("RGB", (400, 120), color=(255, 0, 0)).save(land)
            Image.new("RGB", (120, 400), color=(0, 0, 255)).save(port)
            slots = (
                StripSlotDef(0, 0, 2000, 400, z_index=0),
                StripSlotDef(0, 0, 400, 1200, z_index=1),
            )
            tpl = StripTemplate(
                id="test_smart",
                canvas_width=2400,
                canvas_height=1200,
                slice_width=1080,
                slice_height=1350,
                slice_count=2,
                overlap_px=0,
                background="white",
                slots=slots,
            )
            rng = random.Random(42)
            chosen = pick_smart_fills([land, port], tpl, rng=rng)
            self.assertEqual(len(chosen), 2)
            self.assertIs(chosen[0], land)
            self.assertIs(chosen[1], port)

    def test_prefer_portrait_puts_tall_photo_in_cover_slot(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            land = tmp_path / "land.jpg"
            port = tmp_path / "port.jpg"
            Image.new("RGB", (400, 120), color=(255, 0, 0)).save(land)
            Image.new("RGB", (120, 400), color=(0, 0, 255)).save(port)
            slots = (
                StripSlotDef(0, 0, 2000, 400, z_index=0, prefer_portrait=True),
                StripSlotDef(0, 0, 400, 1200, z_index=1),
            )
            tpl = StripTemplate(
                id="test_cover",
                canvas_width=2400,
                canvas_height=1200,
                slice_width=1080,
                slice_height=1350,
                slice_count=2,
                overlap_px=0,
                background="white",
                slots=slots,
            )
            rng = random.Random(1)
            chosen = pick_smart_fills([land, port], tpl, rng=rng)
            self.assertIs(chosen[0], port)
            self.assertIs(chosen[1], land)

    def test_expand_paths_cyclic_repeats(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            a = tmp_path / "a.jpg"
            b = tmp_path / "b.jpg"
            Image.new("RGB", (10, 10), (1, 2, 3)).save(a)
            Image.new("RGB", (10, 10), (4, 5, 6)).save(b)
            rng = random.Random(0)
            out = expand_paths_cyclic([a, b], 7, rng)
            self.assertEqual(len(out), 7)
            self.assertTrue(all(p in (a, b) for p in out))

    def test_pick_smart_fills_cycles_with_two_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            land = tmp_path / "land.jpg"
            port = tmp_path / "port.jpg"
            Image.new("RGB", (400, 120), color=(255, 0, 0)).save(land)
            Image.new("RGB", (120, 400), color=(0, 0, 255)).save(port)
            slots = tuple(
                StripSlotDef(0, 0, 300 + (i % 5) * 20, 400, z_index=i) for i in range(6)
            )
            tpl = StripTemplate(
                id="test_cycle",
                canvas_width=2400,
                canvas_height=1200,
                slice_width=1080,
                slice_height=1350,
                slice_count=2,
                overlap_px=0,
                background="white",
                slots=slots,
            )
            rng = random.Random(1)
            chosen = pick_smart_fills([land, port], tpl, rng=rng)
            self.assertEqual(len(chosen), 6)
            self.assertTrue(all(c is not None for c in chosen))


class TestStripContainedFit(unittest.TestCase):
    def test_contained_output_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            p = tmp_path / "wide.jpg"
            Image.new("RGB", (800, 200), color=(200, 40, 40)).save(p)
            out = process_image_contained_panned(
                p, 400, 400, "mixed", 0.0, 0.0, fill_rgb=(10, 20, 30)
            )
            self.assertIsNotNone(out)
            assert out is not None
            self.assertEqual(out.size, (400, 400))


class TestStripSeamlessTemplate(unittest.TestCase):
    def test_seamless_export_writes_ten_slices(self) -> None:
        from app.strip import StripImageFill, export_strip_carousel, pick_strip_fills

        tpl = get_template_by_id("strip_seamless_v1")
        self.assertEqual(tpl.num_slots, 9)
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            for i in range(tpl.num_slots):
                Image.new("RGB", (200, 300), color=(i * 25, 50, 80)).save(tmp_path / f"p{i}.jpg")
            paths = sorted(tmp_path.glob("*.jpg"))
            chosen = pick_strip_fills(paths, tpl, smart=False, rng=random.Random(1))
            fills = [StripImageFill(p) for p in chosen]
            wide_p, out = export_strip_carousel(fills, tmp_path / "out", template=tpl)
            self.assertTrue(wide_p.is_file())
            self.assertEqual(len(out), STRIP_SLICE_COUNT)


class TestStripSeamlessMosaicTemplate(unittest.TestCase):
    def test_mosaic_metadata(self) -> None:
        tpl = get_template_by_id("strip_seamless_mosaic_v1")
        self.assertEqual(tpl.layout_placer, "jitter")
        self.assertEqual(len(tpl.slots), len(build_seamless_mosaic_v1_slots(0)))
        self.assertEqual(mosaic_v1_max_slot_count(), 12)
        self.assertEqual(strip_image_slot_count(tpl), 12)
        self.assertEqual(
            strip_image_slot_count(tpl, layout_seed=7),
            len(build_seamless_mosaic_v1_slots(7)),
        )

    def test_mosaic_builder_invariants(self) -> None:
        """Widths sum to 10800; second column (x∈[1000,3200)) is always landscape H / 2H / 3H."""
        cw, ch = 10800, 1350
        for seed in range(64):
            z = build_seamless_mosaic_v1_slots(seed)
            self.assertGreaterEqual(len(z), 9)
            self.assertLessEqual(len(z), 12)
            for s in z:
                self.assertEqual(s.rotation_deg, 0.0)
                self.assertGreaterEqual(s.y, 0)
                self.assertLessEqual(s.y + s.h, ch)
            self.assertEqual(min(s.x for s in z), 0)
            self.assertEqual(max(s.x + s.w for s in z), cw)
            lead = [s for s in z if s.x == 0]
            self.assertEqual(len(lead), 1, msg="first column is always a single V, never 2V")
            self.assertEqual(lead[0].w, 1000)
            self.assertIsNotNone(lead[0].source_trim_left_frac)
            col2 = [s for s in z if s.x >= 1000 and s.x + s.w <= 3200]
            self.assertGreater(len(col2), 0)
            for s in col2:
                self.assertTrue(s.prefer_landscape)
                self.assertTrue(
                    s.cover_height_first or s.horizontal_center_band_frac is not None
                )

    def test_mosaic_resolve_matches_builder(self) -> None:
        tpl = get_template_by_id("strip_seamless_mosaic_v1")
        for seed in (0, 1, 42, 99):
            a = build_seamless_mosaic_v1_slots(seed)
            b = resolve_strip_slots_for_preview(tpl, seed)
            self.assertEqual(a, b)


class TestStripPolaroidTableTemplate(unittest.TestCase):
    def test_polaroid_table_metadata(self) -> None:
        tpl = get_template_by_id("strip_polaroid_table_v1")
        self.assertEqual(tpl.num_slots, 24)
        self.assertEqual(strip_image_slot_count(tpl), 20)
        self.assertIsNotNone(tpl.procedural_background)
        self.assertEqual(tpl.layout_placer, "organic_polaroid")


class TestStripComposeScale(unittest.TestCase):
    def test_compose_scale_halves_canvas(self) -> None:
        from app.strip import StripImageFill

        tpl = get_template_by_id("strip_10col")
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            for i in range(tpl.num_slots):
                Image.new("RGB", (100, 100), (i, 50, 50)).save(tmp_path / f"{i}.jpg")
            paths = sorted(tmp_path.glob("*.jpg"))
            fills = [StripImageFill(p) for p in paths]
            img = compose_strip_wide(fills, tpl, compose_scale=0.5)
            self.assertEqual(img.size, (5400, 675))


def _export_strip_smoke(
    tmp_path: Path,
    template_id: str,
    *,
    layout_seed: int = 42,
    layout_token_retry: bool = False,
) -> tuple[Path, list[Path]]:
    from app.strip import StripImageFill, export_strip_carousel, pick_strip_fills
    from app.strip.assign import pick_underfill_paths
    from app.strip.layout_retry import (
        pick_best_layout_seed_with_token_retry,
        strip_layout_token_retry_enabled,
        underfill_rng_from_layout_seed,
    )

    tpl = get_template_by_id(template_id)
    need = max(3, strip_image_slot_count(tpl) + strip_background_underfill_count(tpl))
    paths = _strip_smoke_source_paths(tmp_path, need)
    rng = random.Random(1)
    chosen = pick_strip_fills(paths, tpl, smart=True, rng=rng, layout_seed=layout_seed)
    fills = [StripImageFill(p) if p is not None else None for p in chosen]
    eff = layout_seed
    if layout_token_retry and strip_layout_token_retry_enabled(tpl.id, "borderless"):
        eff = pick_best_layout_seed_with_token_retry(fills, tpl, layout_seed, card_edge="borderless")
    n_uf = strip_background_underfill_count(tpl)
    uf_rng = underfill_rng_from_layout_seed(eff)
    uf = pick_underfill_paths(paths, chosen, n_uf, uf_rng) if n_uf else None
    return export_strip_carousel(
        fills,
        tmp_path / "out",
        template=tpl,
        layout_seed=eff,
        card_edge="borderless",
        underfill_paths=uf,
    )


class TestStripExportSmoke(unittest.TestCase):
    def test_export_writes_wide_and_slices_per_template(self) -> None:
        self.assertEqual(STRIP_SLICE_COUNT, 10)
        for template_id in STRIP_GUI_TEMPLATE_IDS:
            with self.subTest(template=template_id):
                with tempfile.TemporaryDirectory() as tmp:
                    tmp_path = Path(tmp)
                    wide_p, out = _export_strip_smoke(tmp_path, template_id)
                    self.assertTrue(wide_p.is_file())
                    self.assertIn("_wide", wide_p.name)
                    self.assertEqual(len(out), STRIP_SLICE_COUNT)
                    for p in out:
                        self.assertTrue(p.is_file())
                        self.assertLessEqual(
                            p.stat().st_size,
                            MAX_OUTPUT_JPEG_BYTES,
                            f"{template_id} slice {p.name} exceeds cap",
                        )


class TestStripSeamlessMosaicExportFromTestImages(unittest.TestCase):
    """Mosaic template should export using real ``TEST IMAGES`` when the repo includes that folder."""

    def test_mosaic_export_prefers_test_images_at_repo_root(self) -> None:
        from app.strip import pick_strip_fills

        marker = _project_root() / "TEST IMAGES"
        if not marker.is_dir():
            self.skipTest("TEST IMAGES folder not present")
        tpl = get_template_by_id("strip_seamless_mosaic_v1")
        need = strip_image_slot_count(tpl)
        paths = get_valid_paths(marker, "mixed")
        if len(paths) < need:
            self.skipTest(f"TEST IMAGES needs at least {need} images (top-level), found {len(paths)}")
        marker_res = marker.resolve()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            wide_p, out = _export_strip_smoke(tmp_path, "strip_seamless_mosaic_v1", layout_seed=7)
            self.assertTrue(wide_p.is_file())
            self.assertEqual(len(out), STRIP_SLICE_COUNT)
            chosen = pick_strip_fills(
                paths, tpl, smart=True, rng=random.Random(1), layout_seed=7
            )
            for i, pth in enumerate(chosen):
                if pth is not None:
                    self.assertEqual(
                        pth.resolve().parent,
                        marker_res,
                        msg=f"slot {i} fill should be a top-level file in TEST IMAGES, got {pth}",
                    )


if __name__ == "__main__":
    unittest.main()
