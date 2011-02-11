@echo off
rem Double-click this file to (re)build Mercurial for Windows in place.
rem Useful for testing and development.
cd ..\..
del /Q mercurial\*.pyd
del /Q mercurial\*.pyc
rmdir /Q /S mercurial\locale
python setup.py build_py -c -d . build_ext -i build_mo
pause
