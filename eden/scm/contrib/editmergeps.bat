@echo off
powershell -NoProfile -ExecutionPolicy unrestricted -Command "& '%~dp0\editmergeps.ps1' %*"
