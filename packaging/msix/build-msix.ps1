#Requires -Version 7.0
<#
.SYNOPSIS
Builds a Windows 11 MSIX package and AppInstaller auto-update feed for LeanShell outside the Microsoft Store.
.EXAMPLE
pwsh -File packaging/msix/build-msix.ps1 -Tag v0.0.14 -BinaryDir target/release -OutputDirectory dist/msix -SelfSign
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^v\d+\.\d+\.\d+$')][string] $Tag,
    [Parameter(Mandatory)][string] $BinaryDir,
    [Parameter(Mandatory)][string] $OutputDirectory,
    [string] $Publisher = 'CN=LeanShell Contributors',
    [string] $CertPfx = $env:PFX_PATH,
    [string] $CertPassword = $env:PFX_PASSWORD,
    [switch] $SelfSign
)
$ErrorActionPreference = 'Stop'

$version = $Tag.TrimStart('v')
$msixVersion = "$version.0"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '../..')
$binRoot = Resolve-Path $BinaryDir
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$outRoot = Resolve-Path $OutputDirectory

$stage = Join-Path $outRoot "stage-$version"
$assetsDir = Join-Path $stage 'Assets'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $assetsDir | Out-Null

foreach ($bin in 'leanshell.exe', 'leanshell-settings.exe') {
    $src = Join-Path $binRoot $bin
    if (-not (Test-Path $src)) { throw "Missing binary: $src" }
    Copy-Item $src (Join-Path $stage $bin) -Force
}
if (Test-Path (Join-Path $binRoot 'leanshell-setup.exe')) {
    Copy-Item (Join-Path $binRoot 'leanshell-setup.exe') (Join-Path $stage 'leanshell-setup.exe') -Force
}

# Generate PNG tile assets in pure .NET (works identically in pwsh 7 and Windows PowerShell).
function Write-BrandPng {
    param([string]$Path, [int]$Width, [int]$Height)

    function Get-Crc32([byte[]]$Data) {
        [uint64]$crc = 0xFFFFFFFFL
        foreach ($b in $Data) {
            $crc = $crc -bxor $b
            for ($i = 0; $i -lt 8; $i++) {
                if (($crc -band 1) -ne 0) { $crc = ($crc -shr 1) -bxor 0xEDB88320L }
                else { $crc = $crc -shr 1 }
            }
        }
        return [uint32]($crc -bxor 0xFFFFFFFFL)
    }

    function Write-Chunk($Stream, [string]$Type, [byte[]]$Data) {
        $typeBytes = [System.Text.Encoding]::ASCII.GetBytes($Type)
        $lenBytes = [BitConverter]::GetBytes([int32]$Data.Length)
        [Array]::Reverse($lenBytes)
        $crcBuf = New-Object byte[] ($typeBytes.Length + $Data.Length)
        [Array]::Copy($typeBytes, 0, $crcBuf, 0, $typeBytes.Length)
        [Array]::Copy($Data, 0, $crcBuf, $typeBytes.Length, $Data.Length)
        $crcVal = Get-Crc32 $crcBuf
        $crcBytes = [BitConverter]::GetBytes([uint32]$crcVal)
        [Array]::Reverse($crcBytes)
        $Stream.Write($lenBytes, 0, 4)
        $Stream.Write($typeBytes, 0, 4)
        if ($Data.Length -gt 0) { $Stream.Write($Data, 0, $Data.Length) }
        $Stream.Write($crcBytes, 0, 4)
    }

    $stride = 1 + ($Width * 4)
    $raw = New-Object byte[] ($stride * $Height)
    $size = [Math]::Min($Width, $Height)
    $ox = ($Width - $size) / 2.0
    $oy = ($Height - $size) / 2.0
    $r = $size * 0.22
    for ($y = 0; $y -lt $Height; $y++) {
        $rowOff = $y * $stride
        $raw[$rowOff] = 0
        for ($x = 0; $x -lt $Width; $x++) {
            $px = $x + 0.5 - $ox
            $py = $y + 0.5 - $oy
            $inside = ($px -ge 0 -and $px -le $size -and $py -ge 0 -and $py -le $size)
            $alpha = 0
            if ($inside) {
                $dx = [Math]::Max($r - $px, [Math]::Max(0, $px - ($size - $r)))
                $dy = [Math]::Max($r - $py, [Math]::Max(0, $py - ($size - $r)))
                if (($dx * $dx + $dy * $dy) -le ($r * $r)) { $alpha = 255 }
            }
            $off = $rowOff + 1 + ($x * 4)
            if ($alpha -gt 0) {
                $raw[$off] = 0x08; $raw[$off + 1] = 0x91; $raw[$off + 2] = 0xB2; $raw[$off + 3] = $alpha
            }
        }
    }

    $zStream = New-Object System.IO.MemoryStream
    $zStream.WriteByte(0x78); $zStream.WriteByte(0x9C)
    $deflate = New-Object System.IO.Compression.DeflateStream($zStream, [System.IO.Compression.CompressionLevel]::Optimal, $true)
    $deflate.Write($raw, 0, $raw.Length)
    $deflate.Dispose()
    [uint32]$s1 = 1; [uint32]$s2 = 0
    foreach ($b in $raw) {
        $s1 = ($s1 + $b) % 65521
        $s2 = ($s2 + $s1) % 65521
    }
    $adler = [BitConverter]::GetBytes([uint32](($s2 -shl 16) -bor $s1))
    [Array]::Reverse($adler)
    $zStream.Write($adler, 0, 4)
    $idat = $zStream.ToArray()
    $zStream.Dispose()

    $out = [System.IO.File]::Create($Path)
    try {
        $sig = [byte[]](137, 80, 78, 71, 13, 10, 26, 10)
        $out.Write($sig, 0, 8)
        $ihdr = New-Object byte[] 13
        $wb = [BitConverter]::GetBytes([int32]$Width); [Array]::Reverse($wb)
        $hb = [BitConverter]::GetBytes([int32]$Height); [Array]::Reverse($hb)
        [Array]::Copy($wb, 0, $ihdr, 0, 4)
        [Array]::Copy($hb, 0, $ihdr, 4, 4)
        $ihdr[8] = 8; $ihdr[9] = 6
        Write-Chunk $out 'IHDR' $ihdr
        Write-Chunk $out 'IDAT' $idat
        Write-Chunk $out 'IEND' ([byte[]]@())
    } finally {
        $out.Dispose()
    }
}

foreach ($pair in @(
    @{ Name = 'StoreLogo.png'; W = 50; H = 50 },
    @{ Name = 'Square44x44Logo.png'; W = 44; H = 44 },
    @{ Name = 'Square150x150Logo.png'; W = 150; H = 150 },
    @{ Name = 'Wide310x150Logo.png'; W = 310; H = 150 }
)) {
    Write-BrandPng -Path (Join-Path $assetsDir $pair.Name) -Width $pair.W -Height $pair.H
}

if ($SelfSign -and -not $CertPfx) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher -KeyUsage DigitalSignature `
        -FriendlyName 'LeanShell Dev MSIX Signing' -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    $CertPassword = 'leanshell-dev-msix'
    $secure = ConvertTo-SecureString -String $CertPassword -Force -AsPlainText
    $CertPfx = Join-Path $outRoot 'leanshell-dev.pfx'
    Export-PfxCertificate -Cert $cert -FilePath $CertPfx -Password $secure | Out-Null
    Write-Output "Generated local self-signed dev certificate at $CertPfx (install to TrustedPeople / Root in VM)."
}

$manifest = (Get-Content (Join-Path $PSScriptRoot 'AppxManifest.xml.template') -Raw) `
    .Replace('@VERSION@', $version) `
    .Replace('@MSIX_VERSION@', $msixVersion) `
    .Replace('@PUBLISHER@', $Publisher)
Set-Content -Encoding utf8 (Join-Path $stage 'AppxManifest.xml') $manifest

$appInstaller = (Get-Content (Join-Path $PSScriptRoot 'LeanShell.appinstaller.template') -Raw) `
    .Replace('@VERSION@', $version) `
    .Replace('@MSIX_VERSION@', $msixVersion) `
    .Replace('@PUBLISHER@', $Publisher)
$appInstallerPath = Join-Path $outRoot 'LeanShell.appinstaller'
Set-Content -Encoding utf8 $appInstallerPath $appInstaller

$sdkBin = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^\d+\.' } |
    Sort-Object Name -Descending |
    Select-Object -First 1 -ExpandProperty FullName
$makeAppx = if ($sdkBin) { Join-Path $sdkBin 'x64\makeappx.exe' } else { 'makeappx.exe' }
$signTool = if ($sdkBin) { Join-Path $sdkBin 'x64\signtool.exe' } else { 'signtool.exe' }

$msixPath = Join-Path $outRoot "leanshell-$Tag-x86_64.msix"
& $makeAppx pack /o /d $stage /p $msixPath
if ($LASTEXITCODE) { throw "makeappx failed ($LASTEXITCODE)" }

if ($CertPfx -and (Test-Path $CertPfx)) {
    $signArgs = @('sign', '/fd', 'SHA256', '/f', $CertPfx)
    if ($CertPassword) { $signArgs += @('/p', $CertPassword) }
    $signArgs += $msixPath
    & $signTool @signArgs
    if ($LASTEXITCODE) { throw "signtool failed ($LASTEXITCODE)" }
}

Write-Output "Created MSIX: $msixPath and AppInstaller: $appInstallerPath"
