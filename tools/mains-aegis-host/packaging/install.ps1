$ErrorActionPreference = "Stop"

$Prefix = if ($env:PREFIX) { $env:PREFIX } else { "$env:ProgramFiles\MainsAegis" }
$BinDir = if ($env:BINDIR) { $env:BINDIR } else { Join-Path $Prefix "bin" }
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Copy-Item (Join-Path $ScriptDir "bin\mains-aegis.exe") (Join-Path $BinDir "mains-aegis.exe") -Force
Copy-Item (Join-Path $ScriptDir "bin\mains-aegis-devd.exe") (Join-Path $BinDir "mains-aegis-devd.exe") -Force

Write-Host "Installed mains-aegis and mains-aegis-devd to $BinDir"
