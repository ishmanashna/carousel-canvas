# Ten-seat TEST IMAGES recipe

Fill GPU idle by running 10 Composer seats on photos that are not the DONE five. Model: `composer-2.5` (not fast). Do not launch until asked.

DONE (skipped via `DONE_CUTOUTS`): MIII1129, MIII1196, MIII1136, MIII1320, MIII1143.

```powershell
powershell -File tools\subject-extract\run-folder.ps1 `
  -InputDir "TEST IMAGES" `
  -OutputDir output\subject_extract\ten_seats_staged `
  -Include MIII1132,MIII1152,MIII1178,MIII1192,MIII1213,MIII1233,MIII1253,MIII1272,MIII1300,MIII1319 `
  -Seats 10 `
  -Model composer-2.5 `
  -TimeoutSeconds 2400
```

Watch RAM with 10 Composer processes, and `gpu_lock_wait_seconds` in each `ACTS.jsonl`.
