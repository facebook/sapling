# A simple script for opening merge conflicts in editor
# A loose translation of contrib/editmerge to powershell
# Please make sure that both editmergeps.bat and editmerge.ps1 are available
# via %PATH% and use the following Mercurial settings to enable it
#
# [ui]
# editmergeps
# editmergeps.args=$output
# editmergeps.check=changed
# editmergeps.premerge=keep

$file=$args[0]

function Get-Lines
{
  Select-String "^<<<<<<" $file | % {"$($_.LineNumber)"}
}

$ed = $Env:HGEDITOR;
if ($ed -eq $nil)
{
  $ed = $Env:VISUAL;
}
if ($ed -eq $nil)
{
  $ed = $Env:EDITOR;
}
if ($ed -eq $nil)
{
  $ed = $(hg showconfig ui.editor);
}
if ($ed -eq $nil)
{
  Write-Error "merge failed - unable to find editor"
  exit 1
}

if (($ed -eq "vim") -or ($ed -eq "emacs") -or `
    ($ed -eq "nano") -or ($ed -eq "notepad++"))
{
  $lines = Get-Lines
  $firstline = if ($lines.Length -gt 0) { $lines[0] } else { $nil }
  $previousline = $nil;


  # open the editor to the first conflict until there are no more
  # or the user stops editing the file
  while (($firstline -ne $nil) -and ($firstline -ne $previousline))
  {
    if ($ed -eq "notepad++")
    {
        $linearg = "-n$firstline"
    }
    else
    {
        $linearg = "+$firstline"
    }

    Start-Process -Wait -NoNewWindow $ed $linearg,$file
    $previousline = $firstline
    $lines = Get-Lines
    $firstline = if ($lines.Length -gt 0) { $lines[0] } else { $nil }
  }
}
else
{
  & "$ed" $file
}

$conflicts=Get-Lines
if ($conflicts.Length -ne 0)
{
  Write-Output "merge failed - resolve the conflicts (line $conflicts) then use 'hg resolve --mark'"
  exit 1
}

exit 0

