@REM Copyright (c) Meta Platforms, Inc. and affiliates.
@REM
@REM This software may be used and distributed according to the terms of the
@REM GNU General Public License version 2.

@echo off
SETLOCAL
if exist %~dp0\..\..\opensource\fbcode_builder\getdeps.py (
  set GETDEPS=%~dp0\..\..\opensource\fbcode_builder\getdeps.py
) else if exist %~dp0\build\fbcode_builder\getdeps.py (
  set GETDEPS=%~dp0\build\fbcode_builder\getdeps.py
) else (
  echo "error: unable to find getdeps.py"
  exit 1
)

python3.exe %GETDEPS% build eden %*
