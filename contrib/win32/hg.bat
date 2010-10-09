@echo off
rem Windows Driver script for Mercurial

setlocal
set HG=%~f0

rem Use a full path to Python (relative to this script) as the standard Python
rem install does not put python.exe on the PATH...
rem %~dp0 is the directory of this script

"%~dp0..\python" "%~dp0hg" %*
endlocal
