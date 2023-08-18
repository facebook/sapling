# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

Param (
    # Skip initialization of repo
    [switch]
    $SkipInitialize,

    # Skip crawling repo
    [switch]
    $SkipCrawl,

    # Type of overlay to use when cloning fbsource. Can be "sqlite" or "inmemory".
    [Parameter(Mandatory=$true)]
    [string]
    $OverlayType,

    # Total number of bytes of file to read.
    [long]
    $TotalBytes = 1024*1024,

    # Number of iterations of `hg update` to run.
    [int]
    $Iterations = 10
)

$Repo = "fbsource"
$BasePath = "c:\open\test"
$RepoPath = "$BasePath\$Repo"
$NewCommit = "e24c35e06c240dc214ac8111788d2fea58088bf1" # 8/15/2023
$OldCommit = "35a2e207d1f774d36a03323129531006b5e11cbd" # 2/14/2023 - .~2600000

Write-Progress -Activity "Measure Update" -PercentComplete 0
Push-Location | Out-Null

# Stop and disable chef, soloctl and Defender
$ArgumentList = '{0} {2}; {0} {3}; {0} """"{4}""""; {1} {2}; {1} {3}; {1} """"{4}"""";' `
    -f "Stop-ScheduledTask", "Disable-ScheduledTask", "chefctl-rb", "\Chef\soloctl", "\Microsoft\Windows\Windows Defender\Windows Defender Scheduled Scan"
Start-Process -FilePath powershell -Verb RunAs -WindowStyle Hidden -ArgumentList $ArgumentList

# Initialize repo
if (-not $SkipInitialize) {
    Write-Progress -Activity "Initialization" -PercentComplete 0
    if (Test-Path -Path $RepoPath) {
        Set-Location -Path C:\open\test | Out-Null

        Write-Progress -Activity "Initialization" -Status "Removing Repo" -PercentComplete 5
        & edenfsctl remove -y "$RepoPath" | Out-Null
        Write-Progress -Activity "Initialization" -Status "Removed Repo" -PercentComplete 25

        # Sometimes eden cannot fully clean up
        if (Test-Path -Path $RepoPath) {
            Remove-Item -Path $RepoPath -Recurse -Force | Out-Null
            Write-Progress -Activity "Initialization" -Status "Removed Directory" -PercentComplete 45
        }
    }

    Write-Progress -Activity "Initialization" -Status "Cloning Repo" -PercentComplete 50
    & edenfsctl clone --overlay-type "$OverlayType" "C:\open\eden-backing-repos\$Repo"  "c:\open\test\$Repo" | Out-Null
    Write-Progress -Activity "Initialization" -Completed -PercentComplete 100
}

# Crawl directories, reading files along the way
Write-Progress -Activity "Measure Update" -PercentComplete 25
Set-Location -Path $RepoPath\fbcode\admarket | Out-Null
$BytesRemaining = $TotalBytes
hg update $NewCommit | Out-Null
if (-not $SkipCrawl) {
    Write-Progress -Activity "Crawling" -PercentComplete 0
    $CrawlPercent = 0
    Get-ChildItem -Recurse |
        ForEach-Object {
            $CrawlPercent = (($TotalBytes - $BytesRemaining) * 100 / $TotalBytes)
            Write-Progress -Activity "Crawling" -Status "Reading Files" -PercentComplete $CrawlPercent
            $BytesRead = [Math]::Min([long]$_.Length, $BytesRemaining)
            Get-Content -Path $_.FullName -ReadCount $BytesRead -ErrorAction SilentlyContinue -ErrorVariable GetContentError | Out-Null
            if (-not $GetContentError) {
                $BytesRemaining -= $BytesRead
                if ($BytesRemaining -eq 0L) {
                    return "Done"
                }
            }
        } |
        Select-Object -First 1 | Out-Null
    Write-Progress -Activity "Crawling" -Completed -PercentComplete 100
}

# Restart eden
Set-Location C:\ | Out-Null
eden restart --force | Out-Null

# Measure update commands
Write-Progress -Activity "Measure Update" -PercentComplete 50
Write-Progress -Activity "Updating" -PercentComplete 0
Set-Location $RepoPath | Out-Null
$Results = @()
for ($i = 0; $i -lt $Iterations; $i++) {
    Write-Progress -Activity "Updating" -Status "Iteration $i" -PercentComplete (($i * 100) / $Iterations)
    $Results += Measure-Command -Expression { hg update $OldCommit }
    Write-Progress -Activity "Updating" -Status "Iteration $i" -PercentComplete ((($i * 100) + 50) / $Iterations)
    $Results += Measure-Command -Expression { hg update $NewCommit }
    Write-Progress -Activity "Updating" -Status "Iteration $i" -PercentComplete ((($i + 1) * 100) / $Iterations)
}
Write-Progress -Activity "Updating" -Completed -PercentComplete 100

Write-Output "" # force newline
Write-Output "Measure-Update resutls for '$OverlayType' with '$TotalBytes' bytes crawled:"
$Results | Format-Table -Property TotalSeconds
Pop-Location | Out-Null

# Enable chef, soloctl and Defender
$ArgumentList = '{0} {1}; {0} {2}; {0} """"{3}""""' `
    -f "Enable-ScheduledTask", "chefctl-rb", "\Chef\soloctl", "\Microsoft\Windows\Windows Defender\Windows Defender Scheduled Scan"
Start-Process -FilePath powershell -Verb RunAs -WindowStyle Hidden -ArgumentList $ArgumentList

Write-Progress -Activity "Measure Update" -Completed -PercentComplete 100
