# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

function Is-Admin {
    $currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Start-EdenFS {
    Param($Service)

    if (Is-Admin) {
        Start-Service $Service
    } else {
        $serviceName = $edenfsService.Name
        $command = "Start-Service", "$serviceName"
        $powershellPath = Get-Command powershell.exe

        Start-Process $powershellPath -ArgumentList $command -Wait -Verb RunAs -WindowStyle Hidden

        if ($Service.Status -ne "Running") {
            exit 1
        }
    }
}

function Start-Foreground {
    Start-Process "edenfs.exe" -ArgumentList "--foreground" -WindowStyle Hidden
}

Write-Output "Starting EdenFS service ..."

$edenfsService = Get-Service "edenfs_*"
if ($edenfsService) {
    if ($edenfsService.Status -eq "Running") {
        Write-Output "EdenFS is already running."
        exit
    } else {
        Start-EdenFS -Service $edenfsService
    }
} else {
    # Use a wildcard to avoid Get-Service erroring out.
    $mainService = Get-Service "edenfs*"
    if ($mainService) {
        Write-Warning "Couldn't start the EdenFS service as it was recently installed."
        Write-Warning "When you next login it will be registered. I will continue and spawn it outside of the service manager for now!"
    } else {
        Write-Warning "EdenFS doesn't appear to have been installed properly. Run:"
        Write-Warning "choco uninstall fb.eden"
        Write-Warning "choco install fb.eden"
    }

    Write-Warning "Attempting to start EdenFS manually."
    Start-Foreground
}
