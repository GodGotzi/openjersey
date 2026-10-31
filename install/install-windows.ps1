# Builds OpenJersey, adds it to the Start menu with its icon, and sets up its
# configuration folder (%APPDATA%\openjersey): settings.toml if there is none,
# and the example themes in themes\. Files that already exist there are never
# overwritten. The shortcut starts the binary in this checkout's target\
# folder, so after `git pull; cargo build --release` it starts the new version.
#
#   .\install\install-windows.ps1               install (or update)
#   .\install\install-windows.ps1 -NoThemes     the same, without the example themes
#   .\install\install-windows.ps1 -Remove       remove the shortcut and icon (your configuration stays)
#
# If PowerShell refuses to run it, start it like this:
#   powershell -ExecutionPolicy Bypass -File .\install\install-windows.ps1
param(
    [switch]$Remove,
    [switch]$NoThemes
)
$ErrorActionPreference = 'Stop'

$Repo = Split-Path -Parent $PSScriptRoot
$Bin = Join-Path $Repo 'target\release\openjersey.exe'
$Shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\OpenJersey.lnk'
$IconDir = Join-Path $env:LOCALAPPDATA 'OpenJersey'
$Icon = Join-Path $IconDir 'openjersey.ico'

if ($Remove) {
    Remove-Item -Force -ErrorAction SilentlyContinue $Shortcut
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $IconDir
    Write-Host 'OpenJersey removed from the Start menu (the checkout, its build and your settings are untouched).'
    exit 0
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error 'Rust is missing: install it from https://rustup.rs and run this again.'
}

Write-Host 'Building OpenJersey (the first build takes a few minutes) ...'
Push-Location $Repo
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'The build failed.' }
} finally {
    Pop-Location
}

Write-Host 'Setting up the configuration ...'
# the program makes the folder, settings.toml and themes\ and tells us where they are
$Config = (& $Bin --config-dir).Trim()
if (-not $NoThemes) {
    foreach ($theme in Get-ChildItem (Join-Path $Repo 'examples\themes\*.toml')) {
        $target = Join-Path $Config "themes\$($theme.Name)"
        if (Test-Path $target) {
            Write-Host "  keeping your $($theme.Name)"
        } else {
            Copy-Item $theme.FullName $target
            Write-Host "  added theme $($theme.BaseName)"
        }
    }
}

Write-Host 'Installing the icon ...'
# a Windows icon file can hold a PNG as it is: a small header, then the picture
New-Item -ItemType Directory -Force $IconDir | Out-Null
$png = [System.IO.File]::ReadAllBytes((Join-Path $Repo 'assets\icon.png'))
$stream = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter($stream)
$writer.Write([uint16]0)            # reserved
$writer.Write([uint16]1)            # type: icon
$writer.Write([uint16]1)            # one picture
$writer.Write([byte]0)              # width 256
$writer.Write([byte]0)              # height 256
$writer.Write([byte]0)              # no palette
$writer.Write([byte]0)              # reserved
$writer.Write([uint16]1)            # colour planes
$writer.Write([uint16]32)           # bits per pixel
$writer.Write([uint32]$png.Length)  # size of the picture
$writer.Write([uint32]22)           # where the picture starts
$writer.Write($png)
$writer.Flush()
[System.IO.File]::WriteAllBytes($Icon, $stream.ToArray())

Write-Host 'Adding the Start menu entry ...'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($Shortcut)
$link.TargetPath = $Bin
$link.WorkingDirectory = $Repo
$link.IconLocation = $Icon
$link.Description = 'OpenJersey - design sports kits'
$link.WindowStyle = 7   # the console the program starts with stays minimised
$link.Save()

Write-Host "Done. OpenJersey is in your Start menu; it runs $Bin."
Write-Host "Settings and your own themes live in $Config."
