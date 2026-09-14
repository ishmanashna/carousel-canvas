@echo off
setlocal EnableExtensions
cd /d "%~dp0"

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
set "RELEASE_EXE=%CD%\target\release\carousel-canvas.exe"
set "DEBUG_EXE=%CD%\target\debug\carousel-canvas.exe"

if exist "%RELEASE_EXE%" goto :launch_release
if exist "%DEBUG_EXE%" goto :launch_debug

echo Carousel Canvas is not built yet. Building a release binary...
echo This can take several minutes the first time.
echo.

where cargo >nul 2>&1
if errorlevel 1 (
  echo cargo was not found. Install Rust from https://rustup.rs/
  echo Then run: rustup default stable-msvc
  pause
  exit /b 1
)

set "VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if exist "%VCVARS%" call "%VCVARS%" >nul

cargo build -p app --release
if errorlevel 1 (
  echo.
  echo Build failed.
  pause
  exit /b 1
)

if not exist "%RELEASE_EXE%" (
  echo Build finished but carousel-canvas.exe was not found.
  pause
  exit /b 1
)

:launch_release
start "" "%RELEASE_EXE%"
exit /b 0

:launch_debug
start "" "%DEBUG_EXE%"
exit /b 0
