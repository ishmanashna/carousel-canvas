"""Token layout retry scoring (mural v2)."""

from __future__ import annotations

import unittest

from PIL import Image

from app.strip.layout_retry import score_token_strip_layout


class TestLayoutRetryScore(unittest.TestCase):
    def test_mixed_colors_score_higher_than_uniform(self) -> None:
        bg = (236, 232, 227)
        w, h = 1000, 135
        flat = Image.new("RGB", (w, h), bg)
        s_flat = score_token_strip_layout(
            flat,
            canvas_w=10800,
            canvas_h=1350,
            slice_w=1080,
            slice_h=1350,
            slice_count=10,
            overlap_px=0,
            bg_rgb=bg,
        )
        varied = Image.new("RGB", (w, h), bg)
        px = varied.load()
        for k in range(10):
            for x in range(k * 100, min(w, (k + 1) * 100)):
                for y in range(0, h, 2):
                    px[x, y] = (40 + k * 18, 70 + k * 5, 100 + k * 11)
        s_var = score_token_strip_layout(
            varied,
            canvas_w=10800,
            canvas_h=1350,
            slice_w=1080,
            slice_h=1350,
            slice_count=10,
            overlap_px=0,
            bg_rgb=bg,
        )
        self.assertGreater(s_var, s_flat)


if __name__ == "__main__":
    unittest.main()
