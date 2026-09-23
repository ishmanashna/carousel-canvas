# Second triage — two piles

Look-only. No GPU. No rembg, SAM, wipe, lasso, or lift. This pass sorts a from-scratch second cutout. There is no third cutout.

Ignore the cutter agent’s WORKS/PARTIAL. Rank what you see on this photo only.

Piles are only `good` and `needs_cleanup`. Do not write `redo` or `wont_work`.

Most photos belong in `needs_cleanup`. `good` is the exception: the keeper is intact, and the only extras are a rough edge or a tiny speck you would leave in a collage.

## Decide

1. **Keeper intact, nothing worth deleting** → `good`. Several complete people in one frame are good. A rough edge or a tiny speck is still good.
2. **Anything else** → `needs_cleanup`. Extra pixels to delete: fused blocks, a mic or stand on an edge, detached islands, fragments of other people, gear you are not keeping. A hole, a missing head, or a broken instrument is still this pile: say so in `problem`, and in `leftover` name only the pixels to delete. If there is nothing to delete, `leftover` says that, and says not to invent the missing part.

A complete extra person is a keeper, not leftover.

## Look at (per photo)

1. **Original** — full source photo. Open it when the cutout leaves you unsure.
2. `work_orig.png` in the cutout folder.
3. Cutout: `cutout_full_checker.png` if present, else `cutout_checker.png`, else `cutout_full.png` / `cutout.png`.

## Write

One file: `BATCH.jsonl` in the batch folder the prompt names. One JSON object per line, one line per photo.

```json
{"stem":"STEM","pile":"needs_cleanup","why":"why this pile","problem":"what is wrong","keep":"who stays","leftover":"what to delete","missing":"","next":""}
```

`pile` is `good` or `needs_cleanup`.

Every line needs `why` and `problem`. `needs_cleanup` needs `leftover`. `missing` and `next` stay empty strings.

You may be given up to five photos. Do all of them.

Bans: no `sx` commands, no GUI, no strip, no commit. PowerShell `;` not `&&`.
