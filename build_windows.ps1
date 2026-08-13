# Optional: one-file .exe (PyInstaller). Install first: pip install -r requirements-dev.txt
# Normal use: run `python run_gui.py` instead.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Stop-Process -Name "CarouselCanvas" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

$py = Join-Path $PSScriptRoot ".venv\Scripts\python.exe"
if (-not (Test-Path $py)) {
    $py = "python"
}

& $py -m PyInstaller `
  --noconfirm `
  --onefile `
  --windowed `
  --name "CarouselCanvas" `
  --hidden-import "PIL._tkinter_finder" `
  "run_gui.py"

Write-Host "Output: dist/CarouselCanvas.exe"
