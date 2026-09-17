"""DirectML / CUDA ONNX providers — same GPU idea as crates/vision."""

from __future__ import annotations

import onnxruntime as ort

from timing import note

_PATCHED = False


def available_providers() -> list[str]:
    return list(ort.get_available_providers())


def preferred_providers() -> list:
    avail = set(available_providers())
    if "DmlExecutionProvider" in avail:
        return [
            (
                "DmlExecutionProvider",
                {
                    "device_id": 0,
                    "disable_metacommands": True,
                },
            ),
            "CPUExecutionProvider",
        ]
    if "CUDAExecutionProvider" in avail:
        return ["CUDAExecutionProvider", "CPUExecutionProvider"]
    return ["CPUExecutionProvider"]


def provider_names(providers: list | None = None) -> list[str]:
    items = providers if providers is not None else preferred_providers()
    names: list[str] = []
    for item in items:
        names.append(item[0] if isinstance(item, tuple) else str(item))
    return names


def accelerated() -> bool:
    names = provider_names()
    return bool(names) and names[0] != "CPUExecutionProvider"


def session_options() -> ort.SessionOptions:
    opts = ort.SessionOptions()
    opts.enable_mem_pattern = False
    opts.enable_cpu_mem_arena = False
    opts.intra_op_num_threads = 1
    opts.inter_op_num_threads = 1
    # Fused DML graphs TDR the 4GB display GPU; run smaller kernels.
    opts.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    return opts


def patch_ort_sessions() -> None:
    """rembg SAM ignores providers=; force the same list on every InferenceSession."""
    global _PATCHED
    if _PATCHED:
        return
    orig = ort.InferenceSession
    providers = preferred_providers()

    def _ctor(*args, **kwargs):  # noqa: ANN002, ANN003
        kwargs.setdefault("providers", providers)
        return orig(*args, **kwargs)

    ort.InferenceSession = _ctor  # type: ignore[misc]
    _PATCHED = True


def note_session_providers(session: object) -> str:
    inner = getattr(session, "inner_session", None)
    if inner is None:
        inner = getattr(session, "encoder", None)
    if inner is None:
        return ""
    try:
        active = list(inner.get_providers())
    except Exception:  # noqa: BLE001 - provider query is best-effort
        return ""
    joined = ",".join(active)
    note("ort_providers", joined)
    print(f"ort providers: {joined}", flush=True)
    return joined
