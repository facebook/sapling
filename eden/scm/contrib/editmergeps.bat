@REM Copyright (c) Meta Platforms, Inc. and affiliates.
@REM
@REM This software may be used and distributed according to the terms of the
@REM GNU General Public License version 2.

@echo off
powershell -NoProfile -ExecutionPolicy unrestricted -Command "& '%~dp0\editmergeps.ps1' '%*'"
