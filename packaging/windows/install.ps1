<#
.SYNOPSIS
Installs, upgrades or removes Odoo Backup as a Windows service.

.DESCRIPTION
Run from the extracted odoo-backup-<version>-x86_64-windows.zip, in PowerShell opened as Administrator:

    powershell -ExecutionPolicy Bypass -File .\install.ps1                # install, or upgrade
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -OpenFirewall  # also open the port in Windows Firewall
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall     # remove; data and settings are kept

The program goes to "C:\Program Files\OdooBackup". Settings (odoo-backup.env), data and logs go to
"C:\ProgramData\OdooBackup", which only Administrators, SYSTEM and the service can open. The service runs as the
virtual account "NT SERVICE\OdooBackup" instead of LocalSystem.
#>
[CmdletBinding()]
param(
    [switch]$Uninstall,
    [switch]$OpenFirewall
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'OdooBackup'
$ProgramDir = Join-Path $env:ProgramFiles 'OdooBackup'
$DataDir = Join-Path $env:ProgramData 'OdooBackup'
$Exe = Join-Path $ProgramDir 'odoo-backup.exe'
$EnvFile = Join-Path $DataDir 'odoo-backup.env'
$FirewallRule = 'Odoo Backup'

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script in PowerShell opened as Administrator.'
}

function Invoke-Native {
    param([string]$Command, [string[]]$Arguments)
    $output = & $Command @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed: $output"
    }
}

function Stop-OdooBackup {
    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($service -and $service.Status -ne 'Stopped') {
        Write-Host 'Stopping the service (running backups get time to record their state)...'
        Stop-Service -Name $ServiceName
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(60))
    }
    return $service
}

if ($Uninstall) {
    if (Stop-OdooBackup) {
        Invoke-Native 'sc.exe' @('delete', $ServiceName)
    }
    Get-NetFirewallRule -DisplayName $FirewallRule -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    if (Test-Path $ProgramDir) {
        Remove-Item -Recurse -Force $ProgramDir
    }
    Write-Host "Odoo Backup was removed. Its data and settings were kept in $DataDir."
    return
}

$source = Join-Path $PSScriptRoot 'odoo-backup.exe'
if (-not (Test-Path $source)) {
    throw "odoo-backup.exe was not found next to this script (in $PSScriptRoot)."
}

$existing = Stop-OdooBackup
New-Item -ItemType Directory -Force -Path $ProgramDir, $DataDir | Out-Null
Copy-Item -Force $source $Exe
if (-not (Test-Path $EnvFile)) {
    Copy-Item (Join-Path $PSScriptRoot 'odoo-backup.env') $EnvFile
}

if (-not $existing) {
    Write-Host 'Creating the service...'
    New-Service -Name $ServiceName -DisplayName 'Odoo Backup' -StartupType Automatic `
        -BinaryPathName "`"$Exe`" windows-service `"$EnvFile`"" `
        -Description 'Automated backups of Odoo instances, with a web interface.' | Out-Null
    Invoke-Native 'sc.exe' @('config', $ServiceName, 'obj=', "NT SERVICE\$ServiceName")
    # Restart after a crash: after 5 s, 5 s, then 30 s.
    Invoke-Native 'sc.exe' @('failure', $ServiceName, 'reset=', '86400', 'actions=', 'restart/5000/restart/5000/restart/30000')
}

# The data directory holds the encryption key: Administrators (S-1-5-32-544), SYSTEM (S-1-5-18) and the service only.
Invoke-Native 'icacls.exe' @($DataDir, '/inheritance:r', '/grant:r', '*S-1-5-32-544:(OI)(CI)F', '*S-1-5-18:(OI)(CI)F',
    "NT SERVICE\${ServiceName}:(OI)(CI)M", '/Q')

$port = 8080
$bind = Select-String -Path $EnvFile -Pattern '^\s*ODB_BIND\s*=\s*(\S+)' | Select-Object -Last 1
if ($bind) {
    $port = [int](($bind.Matches[0].Groups[1].Value.Trim('"', "'")) -replace '^.*:', '')
}
if ($OpenFirewall -and -not (Get-NetFirewallRule -DisplayName $FirewallRule -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -DisplayName $FirewallRule -Direction Inbound -Action Allow -Protocol TCP -LocalPort $port `
        -Program $Exe | Out-Null
}

Write-Host 'Starting the service...'
Start-Service -Name $ServiceName
$healthy = $false
foreach ($attempt in 1..30) {
    try {
        Invoke-RestMethod -UseBasicParsing -TimeoutSec 2 -Uri "http://127.0.0.2:$port/api/health" | Out-Null
        $healthy = $true
        break
    } catch {
        Start-Sleep -Seconds 1
    }
}
if (-not $healthy) {
    $log = Get-ChildItem (Join-Path $DataDir 'logs') -Filter 'odoo-backup*.log' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if ($log) {
        Get-Content $log.FullName -Tail 20 | Write-Host
    }
    throw "The service does not answer on port $port. Logs: $DataDir\logs"
}

$version = (& $Exe --version) -replace '^odoo-backup ', ''
Write-Host ''
Write-Host "Odoo Backup $version is running as the $ServiceName service."
Write-Host "  Web interface: http://localhost:$port (create the admin account on the first visit)"
Write-Host "  Settings:      $EnvFile, then: Restart-Service $ServiceName"
Write-Host "  Data:          $DataDir (back up secret.key!)"
Write-Host "  Logs:          $DataDir\logs"
