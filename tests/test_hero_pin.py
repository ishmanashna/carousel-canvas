"""Strip flagship (hero) path resolution and CLI fill pinning."""

from __future__ import annotations

import random
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from app.strip.fills import StripImageFill
from app.strip.hero_pin import pin_strip_hero_fill, resolve_strip_hero_argument
from app.strip.template_data import get_template_by_id, strip_effective_fill_required


class TestResolveStripHero(unittest.TestCase):
    def test_substring_unique(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            Image.new("RGB", (4, 4), (1, 2, 3)).save(d / "MIII4789-Enhanced-NR.jpg")
            Image.new("RGB", (4, 4), (4, 5, 6)).save(d / "other.jpg")
            p = resolve_strip_hero_argument(d, "MIII4789-Enhanced-NR")
            self.assertEqual(p.name, "MIII4789-Enhanced-NR.jpg")

    def test_ambiguous_raises(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp)
            Image.new("RGB", (4, 4), (1, 2, 3)).save(d / "foo-MIII-x.jpg")
            Image.new("RGB", (4, 4), (4, 5, 6)).save(d / "bar-MIII-y.jpg")
            with self.assertRaises(ValueError) as ctx:
                resolve_strip_hero_argument(d, "MIII")
            self.assertIn("ambiguous", str(ctx.exception).lower())


class TestPinStripHero(unittest.TestCase):
    def test_pins_flagship_slot_mural_v2(self) -> None:
        tpl = get_template_by_id("strip_mural_v2")
        hi = tpl.layout_flagship_slot_index
        assert hi is not None
        req = strip_effective_fill_required(tpl, 0)
        n = len(req)
        fills: list[StripImageFill | None] = [
            StripImageFill(Path(f"/tmp/fake{i}.jpg")) for i in range(n)
        ]
        hero = Path("/tmp/HERO_ONLY.jpg")
        pin_strip_hero_fill(
            fills,
            tpl,
            hero,
            layout_seed=0,
            pool_paths=[Path(f"/tmp/fake{i}.jpg") for i in range(n)],
            allow_repeats=True,
            rng=random.Random(1),
        )
        self.assertEqual(fills[hi].path, hero)


if __name__ == "__main__":
    unittest.main()
