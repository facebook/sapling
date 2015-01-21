@echo off
rem Windows Driver script for Mercurial

setlocal
set HG=%~f0

rem Use a full path to Python (relative to this script) if it exists,
rem as the standard Python install does not put python.exe on the PATH...
rem Otherwise, expect that python.exe can be found on the PATH.
rem %~dp0 is the directory of this script

if exist "%~dp0..\python.exe" (
    "%~dp0..\python" "%~dp0hg" %*
) else (
    python "%~dp0hg" %*
)
endlocal

exit /b %ERRORLEVEL%
