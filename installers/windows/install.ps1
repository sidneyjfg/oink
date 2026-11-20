<#
.SYNOPSIS
  Instala o Oink no Windows a partir do MSI oficial das Releases do projeto original.

.DESCRIPTION
  O fork Linux não altera o código do Windows, então o app oficial é compatível com o
  receptor Linux (mesmo protocolo). O script baixa o MSI, confere o SHA-256 publicado
  junto e instala. Se não houver hash publicado, recusa, a menos que use -AllowUnverified.

  O MSI não é assinado. Com o "Controle de Aplicativos Inteligente" (Smart App Control)
  ligado, o Windows bloqueia a instalação e só dá para desligá-lo, em geral sem volta.
  O script avisa, mas nunca mexe nessa configuração.

.EXAMPLE
  .\install.ps1
  .\install.ps1 -Version 0.3.7
  .\install.ps1 -MsiPath .\Oink-0.3.7-windows-x64.msi
#>
[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$MsiPath,
    [string]$Repo = 'sidneyjfg/oink',
    [switch]$AllowUnverified
)
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-SmartAppControlState {
    try {
        (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -ErrorAction Stop).VerifiedAndReputablePolicyState
    } catch { $null }
}

$sac = Get-SmartAppControlState
if ($sac -eq 1 -or $sac -eq 2) {
    Write-Warning ("O Controle de Aplicativos Inteligente está ligado (estado $sac). Ele costuma bloquear este MSI " +
        "('A DLL necessária para a instalação não pôde ser executada'). Desligá-lo é decisão sua e geralmente irreversível.")
}

if (-not $MsiPath) {
    $api = if ($Version -eq 'latest') { "https://api.github.com/repos/$Repo/releases/latest" }
           else { "https://api.github.com/repos/$Repo/releases/tags/v$($Version.TrimStart('v'))" }
    Write-Host "Consultando $api"
    $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'oink-installer' }
    $msi = $release.assets | Where-Object { $_.name -like '*windows-x64.msi' } | Select-Object -First 1
    if (-not $msi) { throw "A release $($release.tag_name) não tem um MSI windows-x64." }
    $sums = $release.assets | Where-Object { $_.name -match 'sha256|sums' } | Select-Object -First 1

    $dir = Join-Path ([IO.Path]::GetTempPath()) ("oink-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $dir | Out-Null
    $MsiPath = Join-Path $dir $msi.name
    Write-Host "Baixando $($msi.name)"
    Invoke-WebRequest -Uri $msi.browser_download_url -OutFile $MsiPath -UseBasicParsing

    $actual = (Get-FileHash -Algorithm SHA256 -Path $MsiPath).Hash.ToLowerInvariant()
    $expected = $null
    if ($sums) {
        $text = (Invoke-WebRequest -Uri $sums.browser_download_url -UseBasicParsing).Content
        if ($text -is [byte[]]) { $text = [Text.Encoding]::UTF8.GetString($text) }
        $line = ($text -split "`r?`n") | Where-Object { $_ -match [regex]::Escape($msi.name) } | Select-Object -First 1
        if ($line) { $expected = ($line -split '\s+')[0].ToLowerInvariant() }
    }
    if ($expected) {
        if ($actual -ne $expected) { throw "SHA-256 não confere (esperado $expected, obtido $actual). Não instalei." }
        Write-Host "SHA-256 conferido: $actual"
    } elseif ($AllowUnverified) {
        Write-Warning "Sem hash publicado; seguindo por causa de -AllowUnverified. SHA-256 do arquivo: $actual"
    } else {
        throw "A release não publica SHA-256 para o MSI. Use -AllowUnverified se aceitar o risco (hash: $actual)."
    }
}

if (-not (Test-Path $MsiPath)) { throw "MSI não encontrado: $MsiPath" }
Write-Host "Instalando $MsiPath"
$p = Start-Process -FilePath msiexec.exe -ArgumentList @('/i', "`"$MsiPath`"", '/passive') -Wait -PassThru
if ($p.ExitCode -notin 0, 3010) { throw "msiexec terminou com código $($p.ExitCode)" }
Write-Host 'Pronto. Abra o Oink e, para parear com um notebook Linux, rode no notebook:  oink-pair <código>'
