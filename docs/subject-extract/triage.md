# Triage — four piles

Look-only. No GPU. No rembg, SAM, wipe, lasso, or lift. Composer seats sort finished cutouts so cleanup and from-scratch redo have a queue.

Ignore the cutter agent’s WORKS/PARTIAL. Rank what you see on this photo only. Do not copy another photo’s pile.

Several complete people in one frame is normal and often **good**. Do not send a cutout to cleanup just because more than one person is in it.

## Decide in this order

1. **No usable keeper** → `wont_work`. The cut is scraps: hands, boards, blobs, nobody you would keep. A clean detail (a face, a hand and what it holds, a torso) that matches the frame is not this. A complete person plus junk is not this.
2. **The keeper is damaged** → `redo`. A hole through the face, a missing chunk of the head or hair, a head that does not join the body, or a held instrument that is broken apart (missing head, gap in the neck) on the person you are keeping. Deleting pixels cannot put those back.
3. **The keeper is intact and the rest is extra** → `needs_cleanup`. Lasso off fused blocks, a mic or stand stuck to an edge, detached islands, other people who are only fragments, and gear you are not keeping. You do not need every cable, the whole drum, or the entire desk. If an accessory is only partly there and finishing it is unrealistic, drop the scraps. Do not redo to recover them.
4. **Otherwise** → `good`. Rough edges and a tiny speck are still good. Several complete people in one frame are good. Do not send a cutout to cleanup because the edge is imperfect.

The keeper is the person you would actually use. If one person is whole and the others are fragments, keep that one person and clean up the rest. Do not redo the frame because a background figure is incomplete.

`redo` is only for damage on the keeper. It does not win just because something else in the frame is missing.

No cutout file at all → `redo`, unless the original photo itself has no subject worth keeping, then `wont_work`.

## Look at (per photo)

The prompt lists three paths for each photo:

1. **Original** — full source photo. Open it whenever the cutout leaves you unsure (is that person actually in the frame, is the scene too messy, is the missing bit real).
2. `work_orig.png` in the cutout folder — same picture at working size.
3. Cutout: `cutout_full_checker.png` if present, else `cutout_checker.png`, else `cutout_full.png` / `cutout.png`.

Start with the cutout. Open the original when the pile is not obvious.

## Write

One file for the whole job: `BATCH.jsonl` in the batch folder the prompt names. **One JSON object per line, one line per photo.** No markdown file per photo.

```json
{"stem":"STEM","pile":"good","why":"why this pile","problem":"what is wrong","keep":"who stays","leftover":"","missing":"","next":""}
```

`pile` is one of `good`, `needs_cleanup`, `redo`, `wont_work`.

Every line needs `why` and `problem`.

- `why` — the decision in one sentence.
- `problem` — what is wrong with this cutout. Empty string only when the pile is `good` and nothing is wrong.
- `keep` — who stays.
- `leftover` — what a later cleanup must delete. Required when the pile is `needs_cleanup`. Empty otherwise.
- `missing` — what is absent on the keeper. Required when the pile is `redo`. Empty otherwise.
- `next` — what a from-scratch second cutout must do differently. Required when the pile is `redo`. Empty otherwise.

You may be given up to **five** photos. Do all of them. Do not skip a line.

Bans: no `sx` cutter commands, no GUI, no strip, no commit. PowerShell `;` not `&&`.

## Handoff (runner, not the looker)

The runner reads `BATCH.jsonl` and appends each photo to `TRIAGE.jsonl` plus `queues/<pile>.jsonl`. If the agent exits without a complete file (dropped connection, empty stdout, or a redo/cleanup line missing `next` / `leftover`), that batch is started again, up to 3 tries, before anything is marked `unlabeled`. A later launch also re-queues photos whose latest line is still `unlabeled`.

A later **redo** pass reads `problem`, `missing`, `next`, `keep`, and `leftover` from that photo’s latest line. Later cleanup reads `problem` and `leftover`.
