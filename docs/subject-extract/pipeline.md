# Pipeline

Stages run in order. Each stage reads the previous stage’s files. Prompts stay photo-agnostic; only the paths change per folder.

This pipeline supplies the figures. The in-app figure cut for this canvas stays frozen. The canvas keeps the photographic ground (the original prints). A later grade can make that ground black and white and leave the figures in color, or the reverse. Cutouts stay full color so that grade does not mean cutting again.

One gig is one band: about 60 photos, same night, same lights. Two bands from one night can share a canvas. Split into two canvases only when each band has enough figures (about 30). Wide shots that never became figures are the background pool.

| stage | reads | writes |
| --- | --- | --- |
| 1 Cutout | a folder of originals | one folder per photo, `cutout_full.png`, `compare_finals\` |
| 2 Triage | those cutouts plus the originals | `TRIAGE.jsonl` (last line per stem wins), `piles\<pile>\`, `queues\<pile>.jsonl` |
| 3 Second cutout | originals whose latest pile is `redo`, plus that line’s `problem`, `missing`, `next`, `keep`, `leftover` | a new output folder. Does not overwrite stage 1. No wipe. |
| 4 Triage again | stage 3 cutouts plus the originals | a new triage log. Piles are only `good` and `needs_cleanup`. A damaged keeper is `needs_cleanup` on this pass. There is no third cutout. Stage 2 `good` and `needs_cleanup` stay as they are. |
| 5 Merge cutouts | the `needs_cleanup` preview pngs in the first-round pile folder | `CLEANUP_QUEUE.jsonl`. A text list of paths. A second-round stem points at its stage 3 folder. Every other stem points at its stage 1 folder. No copy of the photos. |
| 6 Cleanup | that queue, plus `problem` and `leftover` | edits the cutout in the folder the queue names. Before the first lasso, copy `cutout.png` to `pre_cleanup.png` in that same folder. |
| 7 Triage after cleanup | the cleaned cutout, the snapshot, and the cleanup brief | `good`, `needs_more_cleanup`, or `revert`. `revert` means the lasso went wrong: put the snapshot back and do not lasso that photo again. |
| 8 Second cleanup | `needs_more_cleanup` only | another lasso pass, in its own folder. `revert` photos stay on the snapshot. |
| 9 Merge cleanups | stage 6 results and stage 8 results | a path list. A second-cleanup stem points at that pass. A `revert` stem points at `pre_cleanup.png`. No copy of the photos. |
| 10 Final triage | the merged cleaned folder, plus stage 2 and stage 4 cutouts that were already `good` and never cleaned | the figures folder. Anything that still is not a figure stays out of that folder. |
| 11 Backgrounds | the originals, once figures are known | a backgrounds folder. Prefer wide shots that did not pass cutout. A photo used as a figure is not a background. |

The pile folders hold one preview png per photo. The cutout stays in the folder where it was made. A queue names that folder. This pipeline does not copy photo directories.

`good` from stage 2 skips the second cutout and the cleanup, and joins at the final triage. `wont_work` is a background candidate, not a figure.

A live run records where it is in `output/subject_extract/PIPELINE.md`.
