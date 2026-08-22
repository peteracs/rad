@echo off
setlocal EnableExtensions DisableDelayedExpansion

if "%~2"=="" (
    echo usage: capture.cmd ^<output.etl^> "^<command line^>" 1>&2
    exit /b 2
)
if not "%~3"=="" (
    echo capture.cmd accepts the complete command line as one quoted argument 1>&2
    exit /b 2
)

set "RAD_MEMORY_TRACE=%~f1"
set "RAD_MEMORY_COMMAND=%~2"
set "RAD_WPR=D:\Program Files\Windows Kits\10\Windows Performance Toolkit\wpr.exe"
if not exist "%RAD_WPR%" (
    echo Windows Performance Recorder was not found at "%RAD_WPR%" 1>&2
    exit /b 3
)

"%RAD_WPR%" -cancel >nul 2>nul
"%RAD_WPR%" -start GeneralProfile -start VirtualAllocation -filemode
if errorlevel 1 exit /b 4

call %RAD_MEMORY_COMMAND%
set "RAD_COMMAND_EXIT=%ERRORLEVEL%"

"%RAD_WPR%" -stop "%RAD_MEMORY_TRACE%"
if errorlevel 1 exit /b 5
exit /b %RAD_COMMAND_EXIT%
