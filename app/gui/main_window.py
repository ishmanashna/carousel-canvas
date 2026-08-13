"""Main Tkinter window — Carousel Canvas (strip / mural mode only)."""

from __future__ import annotations

import json
import logging
import os
import random
import subprocess
import sys
import threading
import time
import tkinter as tk
from dataclasses import dataclass
from pathlib import Path
from tkinter import filedialog, messagebox, simpledialog, ttk
from tkinter.scrolledtext import ScrolledText
from typing import Any

from PIL import Image, ImageDraw, ImageOps, ImageTk

from app.io import get_valid_paths, parse_color
from app.perf.tracker import record_since, span
from app.strip import STRIP_SLICE_COUNT, export_strip_carousel
from app.strip.assign import pick_underfill_paths
from app.strip.composer import compose_strip_wide, resolve_strip_slots_for_preview
from app.strip.layout_retry import (
    pick_best_layout_seed_with_token_retry,
    strip_layout_token_retry_enabled,
    underfill_rng_from_layout_seed,
)
from app.strip.pipeline import pick_strip_fills, validate_strip_unique_sources
from app.strip.template_data import (
    STRIP_TEMPLATE_OPTIONS,
    StripTemplate,
    effective_slot_fill_required,
    get_template_by_id,
    max_strip_slots,
    strip_background_underfill_count,
    strip_effective_fill_required,
    strip_effective_num_slots,
    strip_image_slot_count,
)

logger = logging.getLogger(__name__)


@dataclass
class ManualSlotFill:
    path: Path
    pan_x: float = 0.0
    pan_y: float = 0.0
    flip_h: bool = False
    grayscale: bool = False


THUMB_GRID_COLS = 3
THUMB_TILE_AR_W = 3
THUMB_TILE_AR_H = 2
THUMB_TILE_BG = (0x2B, 0x2B, 0x2B)
THUMB_DISPLAY_CAP = 200
THUMB_BATCH = 22
THUMB_PUMP_INTERVAL_MS = 20
MANUAL_STAGE_DEBOUNCE_MS = 48
STRIP_PREVIEW_MAX_COMPOSE_WIDTH = 4000
RUN_ESTIMATE_DEBOUNCE_MS = 72
SETTINGS_NAME = "settings.json"
MANUAL_UNDO_MAX = 50

COLOR_PRESETS: list[tuple[str, str]] = [
    ("White", "white"),
    ("Black", "black"),
    ("Beige", "beige"),
    ("Ivory", "ivory"),
    ("Gray", "gray"),
    ("Light gray", "lightgray"),
    ("Dark gray", "darkgray"),
    ("Wheat", "wheat"),
    ("Tan", "tan"),
    ("Navy", "navy"),
    ("Maroon", "maroon"),
]


def _app_data_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or str(Path.home())
    d = Path(base) / "CarouselCanvas"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _settings_file() -> Path:
    return _app_data_dir() / SETTINGS_NAME


class MainWindow:
    def __init__(self, root: tk.Tk) -> None:
        _t_init = time.perf_counter()
        self.root = root
        root.title("Carousel Canvas")
        root.minsize(1100, 720)

        self._busy = False
        self._slot_assignments: list[ManualSlotFill | None] = []
        self._stage_down_slot: int | None = None
        self._stage_press_xy: tuple[int, int] = (0, 0)
        self._stage_pan_moved = False
        self._pan_drag_slot: int | None = None
        self._pan_anchor: tuple[float, float] | None = None
        self._pan_live: tuple[float, float] | None = None
        self._swap_pickup_slot: int | None = None
        self._swap_hover_slot: int | None = None
        self._thumb_refs: list[ImageTk.PhotoImage] = []
        self._thumb_queue: list[Path] = []
        self._thumb_after_id: str | None = None
        self._manual_stage_after_id: str | None = None
        self._run_estimate_after_id: str | None = None
        self._stage_image_id: int | None = None
        self._stage_photo: ImageTk.PhotoImage | None = None
        self._drag_thumb_path: Path | None = None
        self._stage_configure_after: str | None = None
        self._thumb_inner_window_id: int | None = None
        self._thumb_catalog_key: tuple[str, str] | None = None
        self._undo_manual_stack: list[list[ManualSlotFill | None]] = []
        self._redo_manual_stack: list[list[ManualSlotFill | None]] = []
        self._last_run_output_dir: Path | None = None
        self._open_output_btn: ttk.Button | None = None

        self.folder_var = tk.StringVar(value=str(Path.cwd()))
        self.output_var = tk.StringVar(value="output")
        self.mode_var = tk.StringVar(value="strip")
        self.color_var = tk.StringVar(value="white")
        self.run_info_var = tk.StringVar(value="")
        self.run_working_var = tk.StringVar(value="")
        self.strip_smart_shuffle_var = tk.BooleanVar(value=True)
        self.strip_allow_photo_repeats_var = tk.BooleanVar(value=False)
        self.strip_hero_only_var = tk.BooleanVar(value=False)
        self.strip_template_var = tk.StringVar(value="strip_mural_v2")
        self.strip_card_edge_var = tk.StringVar(value="borderless")
        self.strip_layout_seed = random.randint(0, 2**31 - 1)
        self._strip_view: tuple[float, float, float, int, int] | None = None
        self._stage_last_configure_size: tuple[int, int] | None = None
        self._strip_wide_cache_key: tuple[Any, ...] | None = None
        self._strip_wide_cache_image: Image.Image | None = None
        self._strip_overlay_slots: tuple | None = None

        self._reset_strip_slots()
        self._build_ui()
        self._wire_traces()
        self._bind_global_mousewheel()
        self._bind_undo_keys()
        self._load_settings()
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)
        record_since("gui.main_window.init", _t_init)

    def _reset_strip_slots(self) -> None:
        self._slot_assignments = [None] * max_strip_slots()
        self._clear_manual_undo()

    def _clear_manual_undo(self) -> None:
        self._undo_manual_stack.clear()
        self._redo_manual_stack.clear()

    def _clone_assignments(self) -> list[ManualSlotFill | None]:
        out: list[ManualSlotFill | None] = []
        for s in self._slot_assignments:
            if s is None:
                out.append(None)
            else:
                out.append(
                    ManualSlotFill(
                        Path(s.path), s.pan_x, s.pan_y, s.flip_h, s.grayscale
                    )
                )
        return out

    def _clone_from_snap(self, snap: list[ManualSlotFill | None]) -> list[ManualSlotFill | None]:
        out: list[ManualSlotFill | None] = []
        for s in snap:
            if s is None:
                out.append(None)
            else:
                out.append(
                    ManualSlotFill(
                        Path(s.path), s.pan_x, s.pan_y, s.flip_h, s.grayscale
                    )
                )
        return out

    def _manual_edit_checkpoint(self) -> None:
        self._undo_manual_stack.append(self._clone_assignments())
        if len(self._undo_manual_stack) > MANUAL_UNDO_MAX:
            self._undo_manual_stack.pop(0)
        self._redo_manual_stack.clear()

    def _manual_undo(self) -> None:
        if not self._undo_manual_stack:
            return
        self._redo_manual_stack.append(self._clone_assignments())
        prev = self._undo_manual_stack.pop()
        self._slot_assignments = self._clone_from_snap(prev)
        self._schedule_manual_stage_paint(immediate=True)
        self._update_run_estimate()
        self.status_var.set("Undo.")

    def _manual_redo(self) -> None:
        if not self._redo_manual_stack:
            return
        self._undo_manual_stack.append(self._clone_assignments())
        if len(self._undo_manual_stack) > MANUAL_UNDO_MAX:
            self._undo_manual_stack.pop(0)
        nxt = self._redo_manual_stack.pop()
        self._slot_assignments = self._clone_from_snap(nxt)
        self._schedule_manual_stage_paint(immediate=True)
        self._update_run_estimate()
        self.status_var.set("Redo.")

    def _bind_undo_keys(self) -> None:
        self.root.bind_all("<Control-z>", self._on_undo_key, add="+")
        self.root.bind_all("<Control-y>", self._on_redo_key, add="+")
        self.root.bind_all("<Control-Z>", self._on_redo_key, add="+")

    def _on_undo_key(self, _event: tk.Event) -> str | None:
        w = self.root.focus_get()
        if w is not None and w.winfo_class() in ("Entry", "TEntry", "Spinbox", "TSpinbox", "Text"):
            return None
        self._manual_undo()
        return "break"

    def _on_redo_key(self, _event: tk.Event) -> str | None:
        w = self.root.focus_get()
        if w is not None and w.winfo_class() in ("Entry", "TEntry", "Spinbox", "TSpinbox", "Text"):
            return None
        self._manual_redo()
        return "break"

    def _build_ui(self) -> None:
        main = ttk.Frame(self.root, padding=6)
        main.grid(row=0, column=0, sticky="nsew")
        self.root.rowconfigure(0, weight=1)
        self.root.columnconfigure(0, weight=1)
        main.rowconfigure(1, weight=1)
        main.columnconfigure(0, weight=3)
        main.columnconfigure(1, weight=6)
        main.columnconfigure(2, weight=1)

        top = ttk.Frame(main)
        top.grid(row=0, column=0, columnspan=3, sticky="ew", pady=(0, 6))
        ttk.Label(top, text="Input").pack(side=tk.LEFT, padx=(0, 4))
        ttk.Entry(top, textvariable=self.folder_var, width=28).pack(side=tk.LEFT, padx=(0, 4))
        ttk.Button(top, text="Browse…", command=self._browse_input).pack(side=tk.LEFT, padx=(0, 12))
        ttk.Label(top, text="Output").pack(side=tk.LEFT, padx=(0, 4))
        ttk.Entry(top, textvariable=self.output_var, width=20).pack(side=tk.LEFT, padx=(0, 4))
        ttk.Button(top, text="Browse…", command=self._browse_output).pack(side=tk.LEFT, padx=(0, 12))

        left_col = ttk.Frame(main)
        left_col.grid(row=1, column=0, sticky="nsew", padx=(0, 4))
        left_col.rowconfigure(0, weight=1)
        left_col.columnconfigure(0, weight=1)

        thumb_shell = ttk.LabelFrame(left_col, text="Photos (scroll)", padding=4)
        thumb_shell.grid(row=0, column=0, sticky="nsew")
        thumb_outer = ttk.Frame(thumb_shell)
        thumb_outer.pack(fill=tk.BOTH, expand=True)
        thumb_outer.rowconfigure(0, weight=1)
        thumb_outer.columnconfigure(0, weight=1)
        self.thumb_canvas = tk.Canvas(thumb_outer, highlightthickness=0, background="#2b2b2b")
        self.thumb_scroll = ttk.Scrollbar(thumb_outer, orient=tk.VERTICAL, command=self.thumb_canvas.yview)
        self.thumb_inner = ttk.Frame(self.thumb_canvas)
        self.thumb_inner.bind(
            "<Configure>",
            lambda _e: self.thumb_canvas.configure(scrollregion=self.thumb_canvas.bbox("all")),
        )
        self._thumb_inner_window_id = self.thumb_canvas.create_window((0, 0), window=self.thumb_inner, anchor="nw")
        self.thumb_canvas.configure(yscrollcommand=self.thumb_scroll.set)
        self.thumb_canvas.bind("<Configure>", self._on_thumb_canvas_configure)
        self.thumb_canvas.grid(row=0, column=0, sticky="nsew")
        self.thumb_scroll.grid(row=0, column=1, sticky="ns")
        for c in range(THUMB_GRID_COLS):
            self.thumb_inner.columnconfigure(c, weight=1, uniform="thc")
        ttk.Button(
            thumb_shell, text="Refresh photos", command=lambda: self._schedule_thumb_reload(force=True)
        ).pack(anchor="w", pady=(4, 0))

        stage_frame = ttk.LabelFrame(
            main,
            text="Mural preview — drag thumbs to slots; Shift+wheel scrolls horizontally",
            padding=4,
        )
        stage_frame.grid(row=1, column=1, sticky="nsew", padx=4)
        stage_frame.rowconfigure(0, weight=1)
        stage_frame.columnconfigure(0, weight=1)
        self.stage = tk.Canvas(stage_frame, highlightthickness=0, bg="#141414")
        self.stage.grid(row=0, column=0, sticky="nsew")
        self._stage_hscroll = ttk.Scrollbar(stage_frame, orient=tk.HORIZONTAL, command=self.stage.xview)
        self._stage_hscroll.grid(row=1, column=0, sticky="ew")
        self._stage_hscroll.grid_remove()
        self.stage.bind("<Configure>", self._on_stage_configure)
        self.stage.bind("<Button-1>", self._on_stage_button1_press)
        self.stage.bind("<B1-Motion>", self._on_stage_b1_motion)
        self.stage.bind("<ButtonRelease-1>", self._on_stage_button1_release)
        self.stage.bind("<Double-Button-1>", self._on_stage_double_click)
        self.stage.bind("<Button-3>", self._on_stage_right_click)

        right = ttk.Frame(main)
        right.grid(row=1, column=2, sticky="nsew", padx=(4, 0))
        right.columnconfigure(0, weight=1)
        right.rowconfigure(0, weight=1)

        mode_style = ttk.LabelFrame(right, text="Strip options", padding=4)
        mode_style.grid(row=0, column=0, sticky="nsew")
        mode_style.columnconfigure(1, weight=1)
        r = 0
        ttk.Label(mode_style, text="Template").grid(row=r, column=0, sticky="w")
        self._strip_template_combo = ttk.Combobox(
            mode_style,
            state="readonly",
            width=34,
            values=[lbl for _, lbl in STRIP_TEMPLATE_OPTIONS],
        )
        self._strip_template_combo.grid(row=r, column=1, sticky="ew", pady=(0, 4))
        self._strip_template_combo.bind("<<ComboboxSelected>>", self._on_strip_template_combo)
        r += 1
        self._strip_shuffle_btn = ttk.Button(
            mode_style, text="Shuffle strip photos", command=self._on_strip_shuffle
        )
        self._strip_shuffle_btn.grid(row=r, column=0, columnspan=2, sticky="ew", pady=(4, 0))
        r += 1
        self._strip_new_layout_btn = ttk.Button(
            mode_style,
            text="New layout seed (same photos)",
            command=self._on_strip_new_layout_seed,
        )
        self._strip_new_layout_btn.grid(row=r, column=0, columnspan=2, sticky="ew", pady=(4, 0))
        r += 1
        self._strip_hero_only_chk = ttk.Checkbutton(
            mode_style,
            text="Replace hero only (thumb → hero; drop on mural → hero)",
            variable=self.strip_hero_only_var,
        )
        self._strip_hero_only_chk.grid(row=r, column=0, columnspan=2, sticky="w", pady=(2, 0))
        r += 1
        self._strip_smart_chk = ttk.Checkbutton(
            mode_style,
            text="Smart shuffle (match photo shape to slot)",
            variable=self.strip_smart_shuffle_var,
        )
        self._strip_smart_chk.grid(row=r, column=0, columnspan=2, sticky="w", pady=(2, 0))
        r += 1
        self._strip_allow_repeats_chk = ttk.Checkbutton(
            mode_style,
            text="Allow repeating photos (folder has fewer files than image slots)",
            variable=self.strip_allow_photo_repeats_var,
            command=self._save_settings,
        )
        self._strip_allow_repeats_chk.grid(row=r, column=0, columnspan=2, sticky="w", pady=(2, 0))
        r += 1
        ttk.Label(mode_style, text="Card edges").grid(row=r, column=0, sticky="w", pady=(4, 0))
        self._strip_card_edge_combo = ttk.Combobox(
            mode_style,
            state="readonly",
            width=34,
            values=("Borderless (transparent wedges)", "Beige wedges (classic)", "Border (uses color below)"),
        )
        self._strip_card_edge_combo.grid(row=r, column=1, sticky="ew", pady=(4, 0))
        self._strip_card_edge_combo.bind("<<ComboboxSelected>>", self._on_strip_card_edge_combo)
        r += 1
        ttk.Label(mode_style, text="Color").grid(row=r, column=0, sticky="w")
        color_wrap = ttk.Frame(mode_style)
        color_wrap.grid(row=r, column=1, sticky="w", pady=4)
        self._color_mbtn = tk.Menubutton(color_wrap, relief=tk.RAISED, width=16, anchor="w")
        self._color_menu = tk.Menu(self._color_mbtn, tearoff=0)
        for lbl, val in COLOR_PRESETS:
            self._color_menu.add_command(label=lbl, command=lambda v=val: self._set_color_preset(v))
        self._color_menu.add_separator()
        self._color_menu.add_command(label="Custom…", command=self._color_custom_dialog)
        self._color_mbtn["menu"] = self._color_menu
        self._color_mbtn.pack(side=tk.LEFT)
        r += 1
        self.status_var = tk.StringVar(value="Ready.")
        ttk.Label(mode_style, textvariable=self.status_var, wraplength=180).grid(
            row=r, column=0, columnspan=2, sticky="w", pady=(12, 0)
        )
        self._sync_color_button_text()
        self._sync_strip_template_combo_display()
        self._sync_strip_card_edge_combo_display()

        run_frame = ttk.LabelFrame(right, text="Run", padding=4)
        run_frame.grid(row=1, column=0, sticky="ew", pady=(10, 0))
        run_frame.columnconfigure(0, weight=1)
        ttk.Label(run_frame, textvariable=self.run_info_var, wraplength=180, justify=tk.LEFT).grid(
            row=0, column=0, sticky="w"
        )
        self.run_progress = ttk.Progressbar(run_frame, mode="determinate", length=180, maximum=1, value=0)
        self.run_progress.grid(row=1, column=0, sticky="ew", pady=(8, 0))
        self.run_progress.grid_remove()
        ttk.Label(run_frame, textvariable=self.run_working_var, wraplength=180, justify=tk.LEFT).grid(
            row=2, column=0, sticky="w", pady=(2, 0)
        )
        ttk.Button(run_frame, text="Shortcuts…", command=self._show_shortcuts_dialog).grid(
            row=3, column=0, sticky="ew", pady=(8, 0)
        )
        ttk.Button(run_frame, text="Run", command=self._on_run).grid(row=4, column=0, sticky="ew", pady=(10, 0))
        self._open_output_btn = ttk.Button(
            run_frame,
            text="Open output folder",
            command=self._open_last_output_folder,
            state="disabled",
        )
        self._open_output_btn.grid(row=5, column=0, sticky="ew", pady=(6, 0))
        self._update_run_estimate(immediate=True)

    def _on_thumb_canvas_configure(self, event: tk.Event) -> None:
        if self._thumb_inner_window_id is None:
            return
        self.thumb_canvas.itemconfigure(self._thumb_inner_window_id, width=max(event.width, 1))

    def _bind_global_mousewheel(self) -> None:
        self.root.bind_all("<MouseWheel>", self._on_global_mousewheel, add="+")
        self.root.bind_all("<Button-4>", self._on_global_mousewheel, add="+")
        self.root.bind_all("<Button-5>", self._on_global_mousewheel, add="+")

    def _on_global_mousewheel(self, event: tk.Event) -> None:
        w = self.root.winfo_containing(event.x_root, event.y_root)
        if w is None:
            return
        delta = getattr(event, "delta", 0) or 0
        if getattr(event, "num", None) == 4:
            delta = 120
        elif getattr(event, "num", None) == 5:
            delta = -120
        if delta == 0:
            return
        steps = int(-delta / 120)
        if steps == 0:
            steps = -1 if delta > 0 else 1
        if self._widget_is_descendant(w, self.thumb_canvas):
            self.thumb_canvas.yview_scroll(steps, "units")
            return
        if self._widget_is_descendant(w, self.stage) and (getattr(event, "state", 0) & 0x0001):
            self.stage.xview_scroll(steps, "units")

    @staticmethod
    def _widget_is_descendant(w: tk.Misc | None, ancestor: tk.Misc) -> bool:
        cur: tk.Misc | None = w
        while cur is not None:
            if cur == ancestor:
                return True
            cur = getattr(cur, "master", None)
        return False

    def _wire_traces(self) -> None:
        self.folder_var.trace_add(
            "write",
            lambda *_: (
                self._on_folder_changed_strip(),
                self._update_run_estimate(),
            ),
        )

    def _on_strip_shuffle(self) -> None:
        self._manual_edit_checkpoint()
        self._strip_refill_from_folder(randomize=True)
        self._schedule_manual_stage_paint(immediate=True)
        self._update_run_estimate()

    def _on_strip_new_layout_seed(self) -> None:
        self.strip_layout_seed = random.randint(0, 2**31 - 1)
        self._clear_strip_wide_preview_cache()
        self._schedule_manual_stage_paint(immediate=True)
        self._update_run_estimate()
        self.status_var.set("New layout seed — same photos, new jitter.")

    def _strip_flagship_slot_index(self) -> int | None:
        tpl = self._strip_tpl()
        idx = getattr(tpl, "layout_flagship_slot_index", None)
        if idx is None or not (0 <= int(idx) < tpl.num_slots):
            return None
        return int(idx)

    def _strip_tpl(self) -> StripTemplate:
        return get_template_by_id(self.strip_template_var.get())

    def _strip_underfill_paths(self, tpl: StripTemplate, layout_seed: int) -> list[Path]:
        n = strip_background_underfill_count(tpl)
        if n <= 0:
            return []
        folder = Path(self.folder_var.get())
        if not folder.is_dir():
            return []
        paths = get_valid_paths(folder, "mixed")
        ns = strip_effective_num_slots(tpl, int(layout_seed))
        main = [self._slot_assignments[i].path if self._slot_assignments[i] else None for i in range(ns)]
        return pick_underfill_paths(paths, main, n, underfill_rng_from_layout_seed(layout_seed))

    def _strip_num_slots(self) -> int:
        return strip_effective_num_slots(self._strip_tpl(), self._strip_layout_seed_int())

    def _strip_layout_seed_int(self) -> int:
        return int(self.strip_layout_seed)

    def _strip_card_edge_internal(self) -> str:
        v = self.strip_card_edge_var.get()
        if v in ("borderless", "wedges", "border"):
            return v
        return "borderless"

    def _sync_strip_card_edge_combo_display(self) -> None:
        order = ("borderless", "wedges", "border")
        cur = self._strip_card_edge_internal()
        try:
            self._strip_card_edge_combo.current(order.index(cur))
        except ValueError:
            self._strip_card_edge_combo.current(0)

    def _on_strip_card_edge_combo(self, _event=None) -> None:
        i = self._strip_card_edge_combo.current()
        order = ("borderless", "wedges", "border")
        if 0 <= i < len(order):
            self.strip_card_edge_var.set(order[i])
        self._clear_strip_wide_preview_cache()
        self._schedule_manual_stage_paint()
        self._save_settings()

    def _clear_strip_wide_preview_cache(self) -> None:
        self._strip_wide_cache_key = None
        self._strip_wide_cache_image = None

    def _strip_preview_compose_scale(self, tpl: StripTemplate) -> float:
        w = tpl.canvas_width
        if w <= 0:
            return 1.0
        return min(1.0, STRIP_PREVIEW_MAX_COMPOSE_WIDTH / float(w))

    def _strip_fills_for_preview_compose(self, tpl: StripTemplate) -> list[ManualSlotFill | None]:
        n = strip_effective_num_slots(tpl, self._strip_layout_seed_int())
        drag = self._pan_drag_slot
        live = self._pan_live
        out: list[ManualSlotFill | None] = []
        for i in range(n):
            f = self._slot_assignments[i] if i < len(self._slot_assignments) else None
            if f is None:
                out.append(None)
            elif drag == i and live is not None:
                out.append(ManualSlotFill(Path(f.path), live[0], live[1], f.flip_h, f.grayscale))
            else:
                out.append(f)
        return out

    def _strip_preview_cache_key(
        self, tpl: StripTemplate, fills: list[ManualSlotFill | None], compose_scale: float
    ) -> tuple[Any, ...]:
        parts: list[Any] = [
            tpl.id,
            round(compose_scale, 8),
            self._strip_card_edge_internal(),
            int(self.strip_layout_seed),
            str(self.color_var.get()) if self._strip_card_edge_internal() == "border" else "",
        ]
        for i, f in enumerate(fills):
            if f is None:
                parts.append((i, None))
                continue
            try:
                mt = f.path.stat().st_mtime_ns
            except OSError:
                mt = -1
            parts.append((i, str(f.path), mt, f.pan_x, f.pan_y, f.flip_h))
        for p in self._strip_underfill_paths(tpl, int(self.strip_layout_seed)):
            try:
                umt = p.stat().st_mtime_ns
            except OSError:
                umt = -1
            parts.append(("uf", str(p), umt))
        return tuple(parts)

    def _sync_strip_template_combo_display(self) -> None:
        tid = self.strip_template_var.get()
        for i, (opt_id, _lbl) in enumerate(STRIP_TEMPLATE_OPTIONS):
            if opt_id == tid:
                self._strip_template_combo.current(i)
                return
        self._strip_template_combo.current(0)

    def _on_strip_template_combo(self, _event=None) -> None:
        i = self._strip_template_combo.current()
        if i < 0:
            return
        new_id = STRIP_TEMPLATE_OPTIONS[i][0]
        if new_id == self.strip_template_var.get():
            return
        self.strip_template_var.set(new_id)
        self._clear_strip_wide_preview_cache()
        self._strip_refill_from_folder(randomize=True)
        self._schedule_thumb_reload(force=True)
        self._schedule_manual_stage_paint(immediate=True)
        self._update_run_estimate()

    def _set_color_preset(self, value: str) -> None:
        self.color_var.set(value)
        self._sync_color_button_text()
        if self._strip_card_edge_internal() == "border":
            self._clear_strip_wide_preview_cache()
            self._schedule_manual_stage_paint()
        self._save_settings()

    def _color_custom_dialog(self) -> None:
        cur = self.color_var.get()
        v = simpledialog.askstring("Custom color", "Name or #RRGGBB:", initialvalue=cur, parent=self.root)
        if v:
            self.color_var.set(v.strip())
            self._sync_color_button_text()
            if self._strip_card_edge_internal() == "border":
                self._clear_strip_wide_preview_cache()
                self._schedule_manual_stage_paint()
            self._save_settings()

    def _sync_color_button_text(self) -> None:
        t = self.color_var.get() or "white"
        self._color_mbtn.config(text=t[:18] + ("…" if len(t) > 18 else ""))

    def _browse_input(self) -> None:
        p = filedialog.askdirectory(initialdir=self.folder_var.get() or ".")
        if p:
            self.folder_var.set(p)
            self._schedule_thumb_reload(force=True)

    def _browse_output(self) -> None:
        p = filedialog.askdirectory(initialdir=self.output_var.get() or ".")
        if p:
            self.output_var.set(p)

    def _cancel_manual_stage_paint(self) -> None:
        if self._manual_stage_after_id:
            try:
                self.root.after_cancel(self._manual_stage_after_id)
            except Exception:
                pass
            self._manual_stage_after_id = None

    def _schedule_manual_stage_paint(self, *, immediate: bool = False) -> None:
        self._cancel_manual_stage_paint()
        if immediate:
            self._paint_stage()
            return
        self._manual_stage_after_id = self.root.after(
            MANUAL_STAGE_DEBOUNCE_MS, self._fire_manual_stage_paint
        )

    def _fire_manual_stage_paint(self) -> None:
        self._manual_stage_after_id = None
        self._paint_stage()

    def _init_strip_mode(self) -> None:
        self._schedule_thumb_reload(force=True)
        self._stage_last_configure_size = None
        need = strip_image_slot_count(self._strip_tpl(), self._strip_layout_seed_int())
        self.status_var.set(
            f"Strip: wide mural ({need} photo slots) → {STRIP_SLICE_COUNT} vertical 1080×1350 slides. "
            "Drag thumb to slot / click thumb for next empty; right-click to clear. "
            "Scrollbar or Shift+wheel scrolls horizontally."
        )
        self._slot_assignments = [None] * max_strip_slots()
        self._clear_manual_undo()
        self._strip_refill_from_folder(randomize=True)
        self._schedule_manual_stage_paint()
        self._update_run_estimate()

    def _strip_refill_from_folder(self, *, randomize: bool) -> None:
        folder = Path(self.folder_var.get())
        if not folder.is_dir():
            return
        paths = get_valid_paths(folder, "mixed")
        tpl = self._strip_tpl()
        req = effective_slot_fill_required(tpl)
        cap = max_strip_slots()
        if len(paths) < 1:
            self._slot_assignments = [None] * cap
            return
        if randomize:
            self.strip_layout_seed = random.randint(0, 2**31 - 1)
            chosen = pick_strip_fills(
                paths,
                tpl,
                smart=bool(self.strip_smart_shuffle_var.get()),
                rng=random.Random(),
                layout_seed=int(self.strip_layout_seed),
            )
        else:
            pool = sorted(paths)
            req_full = strip_effective_fill_required(tpl, self._strip_layout_seed_int())
            eff_n = len(req_full)
            chosen: list[Path | None] = [None] * eff_n
            optional = sorted(
                (i for i in range(tpl.num_slots) if not req[i]),
                key=lambda i: tpl.slots[i].z_index,
            )
            order = (
                [si for si in range(tpl.num_slots) if req[si]]
                + optional
                + [si for si in range(tpl.num_slots, eff_n)]
            )
            for j, si in enumerate(order):
                chosen[si] = pool[j % len(pool)]
        for i in range(cap):
            if i < len(chosen) and chosen[i] is not None:
                self._slot_assignments[i] = ManualSlotFill(
                    chosen[i], 0.0, 0.0, flip_h=False, grayscale=False
                )
            else:
                self._slot_assignments[i] = None

    def _on_folder_changed_strip(self) -> None:
        self._strip_refill_from_folder(randomize=True)
        self._schedule_thumb_reload(force=True)
        self._schedule_manual_stage_paint()
        self._update_run_estimate()

    def _update_run_estimate(self, *, immediate: bool = False) -> None:
        if immediate:
            if self._run_estimate_after_id:
                try:
                    self.root.after_cancel(self._run_estimate_after_id)
                except Exception:
                    pass
                self._run_estimate_after_id = None
            with span("gui.run_estimate"):
                self._update_run_estimate_impl()
            return
        if self._run_estimate_after_id:
            try:
                self.root.after_cancel(self._run_estimate_after_id)
            except Exception:
                pass
        self._run_estimate_after_id = self.root.after(
            RUN_ESTIMATE_DEBOUNCE_MS, self._run_estimate_fire
        )

    def _run_estimate_fire(self) -> None:
        self._run_estimate_after_id = None
        with span("gui.run_estimate"):
            self._update_run_estimate_impl()

    def _update_run_estimate_impl(self) -> None:
        folder = Path(self.folder_var.get())
        if not folder.is_dir():
            self.run_info_var.set("Choose an input folder to see export details.")
            return
        got = len(get_valid_paths(folder, "mixed"))
        tpl = self._strip_tpl()
        ns = strip_effective_num_slots(tpl, self._strip_layout_seed_int())
        req = strip_effective_fill_required(tpl, self._strip_layout_seed_int())
        need = strip_image_slot_count(tpl, self._strip_layout_seed_int())
        filled = sum(1 for i in range(ns) if req[i] and self._slot_assignments[i] is not None)
        if got < 1:
            self.run_info_var.set("Strip needs at least one photo in the folder (H or V).")
        elif filled < need:
            self.run_info_var.set(f"Fill all {need} strip slots to run ({filled}/{need}).")
        else:
            self.run_info_var.set(
                f"Run will write 1 wide master + {STRIP_SLICE_COUNT} slice JPEGs (1080×1350); "
                f"{need} slots (photos repeat if the folder has fewer than {need})."
            )

    def _required_count(self) -> int:
        return strip_image_slot_count(self._strip_tpl(), self._strip_layout_seed_int())

    def _current_thumb_catalog_key(self) -> tuple[str, str] | None:
        folder = Path(self.folder_var.get())
        try:
            resolved = folder.resolve()
        except OSError:
            return None
        if not resolved.is_dir():
            return None
        return (str(resolved), "strip")

    def _schedule_thumb_reload(self, *, force: bool = False) -> None:
        key = self._current_thumb_catalog_key()
        if (
            not force
            and key is not None
            and key == self._thumb_catalog_key
            and not self._thumb_queue
            and self._thumb_after_id is None
            and len(self._thumb_refs) == len(self.thumb_inner.winfo_children())
        ):
            return
        if self._thumb_after_id:
            try:
                self.root.after_cancel(self._thumb_after_id)
            except Exception:
                pass
            self._thumb_after_id = None
        for w in self.thumb_inner.winfo_children():
            w.destroy()
        self._thumb_refs.clear()
        self._thumb_queue.clear()
        if key is None:
            self._thumb_catalog_key = None
            return
        folder = Path(self.folder_var.get())
        paths = get_valid_paths(folder, "mixed")
        if len(paths) > THUMB_DISPLAY_CAP:
            paths = paths[:THUMB_DISPLAY_CAP]
        self._thumb_catalog_key = key
        self._thumb_queue = list(paths)
        self._thumb_after_id = self.root.after(THUMB_PUMP_INTERVAL_MS, self._pump_thumb)

    def _thumb_cell_size(self) -> tuple[int, int]:
        self.root.update_idletasks()
        inner_w = max(self.thumb_inner.winfo_width(), THUMB_GRID_COLS * 90)
        inner_pad = 2 * (THUMB_GRID_COLS + 1)
        cw = max(128, (inner_w - inner_pad) // THUMB_GRID_COLS)
        ch = max(84, int(cw * THUMB_TILE_AR_H / THUMB_TILE_AR_W))
        return cw, ch

    @staticmethod
    def _thumb_pil_to_tile(contained: Image.Image, cw: int, ch: int) -> Image.Image:
        base = Image.new("RGB", (cw, ch), THUMB_TILE_BG)
        ox = (cw - contained.width) // 2
        oy = (ch - contained.height) // 2
        if contained.mode == "RGBA":
            base.paste(contained, (ox, oy), contained.split()[3])
        else:
            base.paste(contained.convert("RGB"), (ox, oy))
        return base

    def _pump_thumb(self) -> None:
        with span("gui.thumb.pump_batch"):
            self._pump_thumb_impl()

    def _pump_thumb_impl(self) -> None:
        self._thumb_after_id = None
        if not self._thumb_queue:
            return
        cw, ch = self._thumb_cell_size()
        for _ in range(THUMB_BATCH):
            if not self._thumb_queue:
                break
            path = self._thumb_queue.pop(0)
            try:
                with Image.open(path) as im:
                    if im.format == "JPEG" or path.suffix.lower() in (".jpg", ".jpeg"):
                        try:
                            im.draft("RGB", (max(cw * 2, 96), max(ch * 2, 96)))
                        except Exception:
                            pass
                    im = ImageOps.exif_transpose(im.copy())
                    mw, mh = max(cw * 3, 320), max(ch * 3, 240)
                    if im.width > mw or im.height > mh:
                        im.thumbnail((mw, mh), Image.Resampling.BILINEAR)
                    contained = ImageOps.contain(im, (cw, ch), method=Image.Resampling.BILINEAR)
                    tile = self._thumb_pil_to_tile(contained, cw, ch)
                    photo = ImageTk.PhotoImage(tile)
            except Exception as e:
                logger.debug("Thumb skip %s: %s", path, e)
                continue
            self._thumb_refs.append(photo)
            lbl = tk.Label(
                self.thumb_inner,
                image=photo,
                borderwidth=0,
                highlightthickness=0,
                relief=tk.FLAT,
                bg="#2b2b2b",
                cursor="hand2",
            )
            lbl.bind("<ButtonPress-1>", lambda e, p=path: self._thumb_press(p))
            lbl.bind("<ButtonRelease-1>", lambda e, p=path: self._thumb_release(e, p))
            idx = len(self._thumb_refs) - 1
            lbl.grid(row=idx // THUMB_GRID_COLS, column=idx % THUMB_GRID_COLS, padx=0, pady=0, sticky="nsew")
        if self._thumb_queue:
            self._thumb_after_id = self.root.after(THUMB_PUMP_INTERVAL_MS, self._pump_thumb)

    def _thumb_press(self, path: Path) -> None:
        self._drag_thumb_path = path

    def _thumb_release(self, event: tk.Event, path: Path) -> None:
        with span("gui.interaction.thumb_release"):
            wx = self.stage.winfo_rootx()
            wy = self.stage.winfo_rooty()
            ex, ey = event.x_root, event.y_root
            if wx <= ex <= wx + self.stage.winfo_width() and wy <= ey <= wy + self.stage.winfo_height():
                slot = self._hit_test_slot(ex - wx, ey - wy)
                if slot is not None:
                    if self.strip_hero_only_var.get():
                        hi = self._strip_flagship_slot_index()
                        if hi is not None:
                            self._assign_to_slot(hi, path)
                        else:
                            self.status_var.set("No hero slot on this template — turn off “Replace hero only”.")
                    else:
                        self._assign_to_slot(slot, path)
                    self._drag_thumb_path = None
                    return
            self._assign_thumb_click(path)
            self._drag_thumb_path = None

    def _assign_thumb_click(self, path: Path) -> None:
        if self.strip_hero_only_var.get():
            hi = self._strip_flagship_slot_index()
            if hi is None:
                self.status_var.set("No hero slot on this template — turn off “Replace hero only”.")
                return
            self._assign_to_slot(hi, path)
            self.status_var.set("Hero photo updated.")
            return
        tpl = self._strip_tpl()
        req_full = strip_effective_fill_required(tpl, self._strip_layout_seed_int())
        eff_n = strip_effective_num_slots(tpl, self._strip_layout_seed_int())
        if tpl.id == "strip_seamless_mosaic_v1":
            for i in range(eff_n):
                if req_full[i] and self._slot_assignments[i] is None:
                    self._assign_to_slot(i, path)
                    return
        else:
            req_base = effective_slot_fill_required(tpl)
            for i in range(tpl.num_slots):
                if req_base[i] and self._slot_assignments[i] is None:
                    self._assign_to_slot(i, path)
                    return
            for i in range(tpl.num_slots):
                if not req_base[i] and self._slot_assignments[i] is None:
                    self._assign_to_slot(i, path)
                    return
        self.status_var.set("All slots are full — right-click a slot on the preview to clear one.")

    def _assign_to_slot(self, slot: int, path: Path) -> None:
        with span("gui.interaction.assign_to_slot"):
            tpl = self._strip_tpl()
            if not (0 <= slot < strip_effective_num_slots(tpl, self._strip_layout_seed_int())):
                return
            if self.strip_hero_only_var.get():
                hi = self._strip_flagship_slot_index()
                if hi is not None and slot != hi:
                    self.status_var.set("Hero-only mode: use thumbnails or drop on the mural to swap the hero.")
                    return
            self._manual_edit_checkpoint()
            self._slot_assignments[slot] = ManualSlotFill(
                path, pan_x=0.0, pan_y=0.0, flip_h=False, grayscale=False
            )
            self._schedule_manual_stage_paint()
            self._update_run_estimate()

    @staticmethod
    def _event_has_swap_modifier(event: tk.Event) -> bool:
        return (getattr(event, "state", 0) & 0x0004) != 0

    @staticmethod
    def _event_has_shift_modifier(event: tk.Event) -> bool:
        return (getattr(event, "state", 0) & 0x0001) != 0

    def _swap_slot_fills(self, a: int, b: int) -> None:
        n = strip_effective_num_slots(self._strip_tpl(), self._strip_layout_seed_int())
        if not (0 <= a < n and 0 <= b < n) or a == b:
            return
        self._manual_edit_checkpoint()
        self._slot_assignments[a], self._slot_assignments[b] = (
            self._slot_assignments[b],
            self._slot_assignments[a],
        )
        for idx in (a, b):
            sf = self._slot_assignments[idx]
            if sf is not None:
                sf.grayscale = False

    def _hit_test_slot(self, px: float, py: float) -> int | None:
        v = self._strip_view
        if v is None:
            return None
        cx = self.stage.canvasx(px)
        cy = self.stage.canvasy(py)
        scale, img_x, img_y, _pw, _ph = v
        tpl = self._strip_tpl()
        oslots = self._strip_overlay_slots if self._strip_overlay_slots is not None else tpl.slots
        order = sorted(range(len(oslots)), key=lambda i: oslots[i].z_index, reverse=True)
        for i in order:
            s = oslots[i]
            x0 = img_x + s.x * scale
            y0 = img_y + s.y * scale
            x1 = img_x + (s.x + s.w) * scale
            y1 = img_y + (s.y + s.h) * scale
            if x0 <= cx < x1 and y0 <= cy < y1:
                return i
        return None

    def _on_stage_double_click(self, event: tk.Event) -> None:
        self._stage_down_slot = None
        self._stage_pan_moved = False
        self._pan_drag_slot = None
        self._pan_anchor = None
        self._pan_live = None
        self._swap_pickup_slot = None
        self._swap_hover_slot = None
        slot = self._hit_test_slot(event.x, event.y)
        if slot is None:
            return "break"
        fill = self._slot_assignments[slot] if slot < len(self._slot_assignments) else None
        if fill is None:
            return "break"
        self._manual_edit_checkpoint()
        if self._event_has_shift_modifier(event):
            self.status_var.set("Strip is always full color — B&W toggle is disabled.")
            return "break"
        fill.flip_h = not fill.flip_h
        self.status_var.set(f"Slot {slot + 1}: horizontal flip {'on' if fill.flip_h else 'off'}.")
        self._schedule_manual_stage_paint(immediate=True)
        return "break"

    def _on_stage_button1_press(self, event: tk.Event) -> None:
        self._swap_hover_slot = None
        slot = self._hit_test_slot(event.x, event.y)
        if self._event_has_swap_modifier(event) and slot is not None:
            fill = self._slot_assignments[slot] if slot < len(self._slot_assignments) else None
            if fill is not None:
                self._swap_pickup_slot = slot
                self._stage_down_slot = None
                self.status_var.set(
                    f"Slot {slot + 1} picked — release on another slot to swap (crops stay with each photo)."
                )
                self._schedule_manual_stage_paint(immediate=True)
                return
        self._swap_pickup_slot = None
        self._stage_down_slot = slot
        self._stage_press_xy = (event.x, event.y)
        self._stage_pan_moved = False
        self._pan_drag_slot = None
        self._pan_anchor = None
        self._pan_live = None

    def _slot_pan_sensitivity(self, slot: int) -> tuple[float, float] | None:
        v = self._strip_view
        tpl = self._strip_tpl()
        oslots = self._strip_overlay_slots if self._strip_overlay_slots is not None else tpl.slots
        if v is None or slot >= len(oslots):
            return None
        scale, _img_x, _img_y, _pw, _ph = v
        s = oslots[slot]
        tws = max(1.0, s.w * scale)
        ths = max(1.0, s.h * scale)
        return 2.0 / tws, 2.0 / ths

    def _on_stage_b1_motion(self, event: tk.Event) -> None:
        if self._swap_pickup_slot is not None:
            self._swap_hover_slot = self._hit_test_slot(event.x, event.y)
            self._schedule_manual_stage_paint(immediate=True)
            return
        slot = self._stage_down_slot
        if slot is None:
            return
        fill = self._slot_assignments[slot] if slot < len(self._slot_assignments) else None
        if fill is None:
            return
        tcx = event.x - self._stage_press_xy[0]
        tcy = event.y - self._stage_press_xy[1]
        if not self._stage_pan_moved:
            if tcx * tcx + tcy * tcy < 36:
                return
            self._stage_pan_moved = True
            self._pan_drag_slot = slot
            self._pan_anchor = (fill.pan_x, fill.pan_y)
        sens = self._slot_pan_sensitivity(slot)
        if sens is None or self._pan_anchor is None:
            return
        sens_x, sens_y = sens
        ax, ay = self._pan_anchor
        self._pan_live = (
            max(-1.0, min(1.0, ax - tcx * sens_x)),
            max(-1.0, min(1.0, ay - tcy * sens_y)),
        )
        self._schedule_manual_stage_paint()

    def _on_stage_button1_release(self, event: tk.Event) -> None:
        if self._swap_pickup_slot is not None:
            src = self._swap_pickup_slot
            tgt = self._hit_test_slot(event.x, event.y)
            self._swap_pickup_slot = None
            self._swap_hover_slot = None
            if tgt is not None and tgt != src:
                self._swap_slot_fills(src, tgt)
                self.status_var.set(f"Swapped slots {src + 1} and {tgt + 1}.")
            else:
                self.status_var.set("Swap cancelled.")
            self._schedule_manual_stage_paint(immediate=True)
            self._update_run_estimate()
            self._stage_down_slot = None
            self._stage_pan_moved = False
            return
        if self._stage_pan_moved and self._pan_drag_slot is not None and self._pan_live is not None:
            fill = self._slot_assignments[self._pan_drag_slot]
            if fill is not None:
                self._manual_edit_checkpoint()
                fill.pan_x, fill.pan_y = self._pan_live
            self._schedule_manual_stage_paint(immediate=True)
        self._pan_drag_slot = None
        self._pan_anchor = None
        self._pan_live = None
        self._stage_down_slot = None
        self._stage_pan_moved = False

    def _on_stage_right_click(self, event: tk.Event) -> None:
        slot = self._hit_test_slot(event.x, event.y)
        if slot is not None:
            self._manual_edit_checkpoint()
            self._slot_assignments[slot] = None
            self._schedule_manual_stage_paint()
            self._update_run_estimate()
            self.status_var.set(f"Slot {slot + 1} cleared — drag or click a thumbnail to refill.")

    def _on_stage_configure(self, _event=None) -> None:
        sw = max(self.stage.winfo_width(), 50)
        sh = max(self.stage.winfo_height(), 50)
        if (sw, sh) == self._stage_last_configure_size:
            return
        self._stage_last_configure_size = (sw, sh)
        if self._stage_configure_after:
            try:
                self.root.after_cancel(self._stage_configure_after)
            except Exception:
                pass
        self._stage_configure_after = self.root.after(120, self._redraw_stage_debounced)

    def _redraw_stage_debounced(self) -> None:
        self._stage_configure_after = None
        self._paint_stage()

    def _paint_stage(self) -> None:
        with span("gui.stage.paint"):
            self._paint_strip_stage()

    def _paint_strip_stage(self) -> None:
        self.stage.delete("all")
        self._stage_image_id = None
        self._stage_photo = None
        sw = max(self.stage.winfo_width(), 50)
        sh = max(self.stage.winfo_height(), 50)
        xs = self.stage.xview()
        tpl = self._strip_tpl()
        n = strip_effective_num_slots(tpl, self._strip_layout_seed_int())
        fills: list = list(self._slot_assignments[:n])
        while len(fills) < n:
            fills.append(None)

        def reset_scroll() -> None:
            self.stage.config(scrollregion=(0, 0, sw, sh))
            self.stage.xview_moveto(0)
            self.stage.yview_moveto(0)

        req = strip_effective_fill_required(tpl, self._strip_layout_seed_int())
        need_empty = [i for i in range(n) if req[i] and fills[i] is None]
        if need_empty:
            self._clear_strip_wide_preview_cache()
            self._strip_overlay_slots = None
            self._strip_view = None
            reset_scroll()
            self._sync_strip_scrollbar(sw, sw)
            self._show_stage_message(
                f"Strip ({tpl.id}): fill {strip_image_slot_count(tpl, self._strip_layout_seed_int())} photo slots — {len(need_empty)} empty."
            )
            return

        effective = self._strip_fills_for_preview_compose(tpl)
        pscale = self._strip_preview_compose_scale(tpl)
        cache_key = self._strip_preview_cache_key(tpl, effective, pscale)
        ce = self._strip_card_edge_internal()
        brgb = parse_color(self.color_var.get()) if ce == "border" else None
        with span("gui.stage.build_strip_image"):
            try:
                if cache_key == self._strip_wide_cache_key and self._strip_wide_cache_image is not None:
                    wide = self._strip_wide_cache_image
                else:
                    slots = resolve_strip_slots_for_preview(tpl, self.strip_layout_seed)
                    wide = compose_strip_wide(
                        effective,
                        tpl,
                        compose_scale=pscale,
                        layout_seed=self.strip_layout_seed,
                        card_edge=ce,  # type: ignore[arg-type]
                        border_rgb=brgb,
                        resolved_slots=slots,
                        underfill_paths=self._strip_underfill_paths(tpl, int(self.strip_layout_seed)),
                    )
                    self._strip_wide_cache_key = cache_key
                    self._strip_wide_cache_image = wide
                    self._strip_overlay_slots = slots
            except Exception as e:
                logger.debug("strip preview compose failed: %s", e)
                self._clear_strip_wide_preview_cache()
                self._strip_overlay_slots = None
                self._strip_view = None
                reset_scroll()
                self._sync_strip_scrollbar(sw, sw)
                self._show_stage_message("Could not build strip preview.")
                return

        tw_s, th_s = wide.size
        scale0 = (sh * 0.92) / float(th_s)
        pw = max(1, int(round(tw_s * scale0)))
        ph = max(1, int(round(th_s * scale0)))
        preview = wide.resize((pw, ph), Image.Resampling.LANCZOS)
        W = max(sw, pw)
        H = max(sh, ph)
        ix = (W - pw) // 2
        iy = (H - ph) // 2
        out = Image.new("RGB", (W, H), (20, 20, 20))
        out.paste(preview, (ix, iy))
        draw = ImageDraw.Draw(out)
        cw = float(tpl.canvas_width)
        ch = float(tpl.canvas_height)
        oslots = self._strip_overlay_slots if self._strip_overlay_slots is not None else tpl.slots
        for si, slot in enumerate(oslots):
            x0 = int(ix + slot.x / cw * pw)
            y0 = int(iy + slot.y / ch * ph)
            x1 = int(ix + (slot.x + slot.w) / cw * pw)
            y1 = int(iy + (slot.y + slot.h) / ch * ph)
            if self._swap_pickup_slot is not None:
                if si == self._swap_pickup_slot:
                    draw.rectangle([x0 - 2, y0 - 2, x1 + 1, y1 + 1], outline="#66ccff", width=2)
                elif self._swap_hover_slot == si and si != self._swap_pickup_slot:
                    draw.rectangle([x0 - 2, y0 - 2, x1 + 1, y1 + 1], outline="#ffaa33", width=3)

        self._stage_photo = ImageTk.PhotoImage(out)
        self.stage.config(scrollregion=(0, 0, W, H))
        self._stage_image_id = self.stage.create_image(0, 0, anchor="nw", image=self._stage_photo)
        vis_scale = pw / cw
        self._strip_view = (vis_scale, float(ix), float(iy), pw, ph)
        try:
            if len(xs) >= 2 and W > sw:
                self.stage.xview_moveto(max(0.0, min(1.0, float(xs[0]))))
            elif pw > sw:
                total = max(1, W - sw)
                self.stage.xview_moveto(max(0.0, min(1.0, ((pw - sw) / 2) / total)))
            else:
                self.stage.xview_moveto(0.0)
            self.stage.yview_moveto(0.0)
        except tk.TclError:
            pass
        self._sync_strip_scrollbar(W, sw)

    def _sync_strip_scrollbar(self, scroll_width: int, viewport_w: int) -> None:
        if scroll_width > viewport_w:
            self._stage_hscroll.grid(row=1, column=0, sticky="ew")
            self.stage.config(xscrollcommand=self._stage_hscroll.set)
        else:
            self._stage_hscroll.grid_remove()
            self.stage.config(xscrollcommand=lambda *_: None)

    def _show_stage_message(self, msg: str) -> None:
        sw = max(self.stage.winfo_width(), 50)
        sh = max(self.stage.winfo_height(), 50)
        self.stage.delete("all")
        self._stage_hscroll.grid_remove()
        try:
            self.stage.config(xscrollcommand=lambda *_: None)
        except tk.TclError:
            pass
        self.stage.config(scrollregion=(0, 0, sw, sh))
        self.stage.xview_moveto(0)
        self.stage.yview_moveto(0)
        self.stage.create_text(
            sw // 2,
            sh // 2,
            text=msg[:280] + ("…" if len(msg) > 280 else ""),
            fill="#aaaaaa",
            width=max(200, self.stage.winfo_width() - 40),
        )

    def _validate_run(self) -> str | None:
        folder = Path(self.folder_var.get())
        if not folder.is_dir():
            return "Input folder does not exist."
        out = Path(self.output_var.get())
        try:
            out.mkdir(parents=True, exist_ok=True)
        except OSError as e:
            return f"Cannot create output folder: {e}"
        tpl = self._strip_tpl()
        req = strip_effective_fill_required(tpl, self._strip_layout_seed_int())
        need = strip_image_slot_count(tpl, self._strip_layout_seed_int())
        if len(get_valid_paths(folder, "mixed")) < 1:
            return "Strip needs at least one photo in the input folder."
        if any(
            self._slot_assignments[i] is None
            for i in range(strip_effective_num_slots(tpl, self._strip_layout_seed_int()))
            if req[i]
        ):
            return f"Fill all {need} strip slots (use Shuffle or drag from thumbnails)."
        fills_check = [
            self._slot_assignments[i]
            for i in range(strip_effective_num_slots(tpl, self._strip_layout_seed_int()))
        ]
        try:
            validate_strip_unique_sources(
                tpl,
                fills=fills_check,
                allow_repeats=bool(self.strip_allow_photo_repeats_var.get()),
                layout_seed=self._strip_layout_seed_int(),
            )
        except ValueError as e:
            return str(e)
        return None

    def _run_progress_report(self, done: int, total: int) -> None:
        total = max(1, total)
        self.run_progress.configure(mode="determinate", maximum=total, value=min(done, total))

    def _log_file_dir(self) -> Path:
        base = os.environ.get("LOCALAPPDATA") or str(Path.home())
        return Path(base) / "CarouselCanvas" / "logs"

    def _reveal_folder(self, folder: Path) -> None:
        try:
            folder = folder.expanduser().resolve()
        except OSError:
            return
        if not folder.is_dir():
            try:
                folder.mkdir(parents=True, exist_ok=True)
            except OSError:
                return
        try:
            if sys.platform == "win32":
                os.startfile(folder)  # type: ignore[attr-defined]
            elif sys.platform == "darwin":
                subprocess.run(["open", str(folder)], check=False)
            else:
                subprocess.run(["xdg-open", str(folder)], check=False)
        except OSError as e:
            logger.warning("Could not open folder %s: %s", folder, e)

    def _open_last_output_folder(self) -> None:
        if self._last_run_output_dir is None:
            return
        self._reveal_folder(self._last_run_output_dir)

    def _show_shortcuts_dialog(self) -> None:
        win = tk.Toplevel(self.root)
        win.title("Shortcuts")
        win.transient(self.root)
        txt = ScrolledText(win, width=72, height=18, wrap="word", font=("Segoe UI", 9))
        txt.pack(fill=tk.BOTH, expand=True, padx=8, pady=8)
        body = """CAROUSEL CANVAS
• Run exports 1 wide master + 10 carousel slice JPEGs (1080×1350).
• Drag thumbnails onto mural slots, or click a thumb to fill the next empty slot.
• Drag on a filled photo to pan; double-click to flip horizontally.
• Ctrl+drag between slots to swap. Right-click clears a slot.
• Ctrl+Z / Ctrl+Y: undo / redo slot edits (up to 50 steps).
• Shift + mouse wheel on the preview scrolls horizontally.
• Shuffle strip photos refills slots and picks a new layout seed.
• Mural v2 + borderless Run tries 5 token layout seeds and picks the best."""
        txt.insert("1.0", body.strip())
        txt.config(state="disabled")
        ttk.Button(win, text="Close", command=win.destroy).pack(pady=(0, 8))

    def _on_run(self) -> None:
        if self._busy:
            return
        err = self._validate_run()
        if err:
            messagebox.showerror("Cannot run", err)
            return
        self._busy = True
        self._run_t0 = time.monotonic()
        try:
            self.run_progress.stop()
        except tk.TclError:
            pass
        self.run_progress.grid(row=1, column=0, sticky="ew", pady=(8, 0))
        self.run_progress.config(mode="determinate", maximum=STRIP_SLICE_COUNT, value=0)
        self.run_working_var.set(
            f"Processing… exporting wide master + {STRIP_SLICE_COUNT} carousel slices."
        )
        self.status_var.set("Working…")
        folder = Path(self.folder_var.get()).resolve()
        out = Path(self.output_var.get())
        strip_tpl_run = self._strip_tpl()

        def progress_report(done: int, total: int) -> None:
            self.root.after(0, lambda d=done, t=total: self._run_progress_report(d, t))

        def work():
            exc: Exception | None = None
            try:
                progress_report(0, STRIP_SLICE_COUNT)
                ce = self.strip_card_edge_var.get()
                if ce not in ("borderless", "wedges", "border"):
                    ce = "borderless"
                brgb = parse_color(self.color_var.get()) if ce == "border" else None
                eff_layout_seed = int(self.strip_layout_seed)
                strip_n = strip_effective_num_slots(strip_tpl_run, eff_layout_seed)
                fills_strip = [self._slot_assignments[i] for i in range(strip_n)]
                if strip_layout_token_retry_enabled(strip_tpl_run.id, ce):
                    eff_layout_seed = pick_best_layout_seed_with_token_retry(
                        fills_strip, strip_tpl_run, eff_layout_seed, card_edge=ce
                    )
                    strip_n = strip_effective_num_slots(strip_tpl_run, eff_layout_seed)
                    fills_strip = [self._slot_assignments[i] for i in range(strip_n)]
                uf_run = self._strip_underfill_paths(strip_tpl_run, eff_layout_seed)
                export_strip_carousel(
                    fills_strip,
                    out,
                    template=strip_tpl_run,
                    progress_callback=progress_report,
                    layout_seed=eff_layout_seed,
                    card_edge=ce,  # type: ignore[arg-type]
                    border_rgb=brgb,
                    underfill_paths=uf_run or None,
                )
            except Exception as e:
                exc = e
                logger.exception("Run failed")
            self.root.after(0, lambda: self._run_done(exc))

        threading.Thread(target=work, daemon=True).start()

    def _run_done(self, exc: Exception | None) -> None:
        self._busy = False
        try:
            self.run_progress.stop()
        except tk.TclError:
            pass
        try:
            self.run_progress.grid_remove()
        except tk.TclError:
            pass
        self.run_working_var.set("")
        elapsed = time.monotonic() - getattr(self, "_run_t0", time.monotonic())
        out_dir = Path(self.output_var.get()).expanduser()
        try:
            out_resolved = out_dir.resolve()
        except OSError:
            out_resolved = out_dir
        if exc:
            self.status_var.set("Failed — see log.")
            self._last_run_output_dir = None
            if self._open_output_btn is not None:
                self._open_output_btn.state(["disabled"])
            msg = str(exc)[:1200]
            if messagebox.askyesno("Run failed", f"{msg}\n\nOpen log folder?"):
                self._reveal_folder(self._log_file_dir())
        else:
            self.status_var.set(f"Finished in {elapsed:.1f}s. Check output folder.")
            self._last_run_output_dir = out_resolved
            if self._open_output_btn is not None:
                self._open_output_btn.state(["!disabled"])

    def _load_settings(self) -> None:
        path = _settings_file()
        if path.is_file():
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
            except Exception:
                data = {}
            else:
                if "input_folder" in data and Path(data["input_folder"]).is_dir():
                    self.folder_var.set(data["input_folder"])
                if "output_folder" in data:
                    self.output_var.set(data["output_folder"])
                if "strip_smart_shuffle" in data:
                    self.strip_smart_shuffle_var.set(bool(data["strip_smart_shuffle"]))
                if "strip_allow_photo_repeats" in data:
                    self.strip_allow_photo_repeats_var.set(bool(data["strip_allow_photo_repeats"]))
                if "strip_template" in data:
                    tid = str(data["strip_template"])
                    try:
                        get_template_by_id(tid)
                    except KeyError:
                        pass
                    else:
                        self.strip_template_var.set(tid)
                if "strip_card_edge" in data:
                    v = str(data["strip_card_edge"])
                    if v in ("borderless", "wedges", "border"):
                        self.strip_card_edge_var.set(v)
                if "color" in data:
                    self.color_var.set(str(data["color"]))
        self._sync_color_button_text()
        self._sync_strip_card_edge_combo_display()
        self._sync_strip_template_combo_display()
        self._init_strip_mode()
        self._update_run_estimate(immediate=True)

    def _save_settings(self) -> None:
        data = {
            "input_folder": self.folder_var.get(),
            "output_folder": self.output_var.get(),
            "strip_smart_shuffle": bool(self.strip_smart_shuffle_var.get()),
            "strip_allow_photo_repeats": bool(self.strip_allow_photo_repeats_var.get()),
            "strip_template": self.strip_template_var.get(),
            "strip_card_edge": self._strip_card_edge_internal(),
            "color": self.color_var.get(),
        }
        try:
            _settings_file().write_text(json.dumps(data, indent=2), encoding="utf-8")
        except OSError as e:
            logger.warning("Could not save settings: %s", e)

    def _on_close(self) -> None:
        self._save_settings()
        self.root.destroy()
