<#
Exercise the real helper in child PowerShell processes with an isolated fake
pnpm command. No package, tool download, installer or real build is executed.
#>
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('neati-nsis-retry-test-' + [guid]::NewGuid())
$originalPath = $env:PATH
$fixtureVariables = @('NEATI_CI_NSIS_CASE', 'NEATI_CI_NSIS_COUNT', 'NEATI_CI_NSIS_ARGS', 'NEATI_CI_NSIS_DELAYS')
$originalVariables = @{}
foreach ($name in $fixtureVariables) {
  $originalVariables[$name] = [Environment]::GetEnvironmentVariable($name)
}

try {
  New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
  $fakePnpm = @'
@echo off
set count=0
if exist "%NEATI_CI_NSIS_COUNT%" set /p count=<"%NEATI_CI_NSIS_COUNT%"
set /a count+=1 >nul
>"%NEATI_CI_NSIS_COUNT%" echo %count%
>>"%NEATI_CI_NSIS_ARGS%" echo %*
if "%NEATI_CI_NSIS_CASE%"=="success" exit /b 0
if "%NEATI_CI_NSIS_CASE%"=="compile-failure" (
  echo error[E0425]: cannot find value 1>&2
  exit /b 23
)
if "%NEATI_CI_NSIS_CASE%"=="other-download-timeout" (
  echo Downloading https://github.com/unrelated/project/releases/download/v1/tool.zip 1>&2
  echo failed to bundle project: `timeout: global` 1>&2
  exit /b 31
)
echo Downloading https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip 1>&2
if "%NEATI_CI_NSIS_CASE%"=="hash-failure" (
  echo failed to bundle project: hash mismatch 1>&2
  exit /b 41
)
if "%NEATI_CI_NSIS_CASE%"=="timeout-then-success" if %count% GEQ 3 exit /b 0
echo failed to bundle project: `timeout: global` 1>&2
exit /b 31
'@
  [System.IO.File]::WriteAllText((Join-Path $fixtureRoot 'pnpm.cmd'), $fakePnpm.Replace("`n", "`r`n"), [System.Text.Encoding]::ASCII)
  # Replace only the wait in this test subprocess, preserving real helper
  # invocation, native command execution, retry classification and exit handling.
  $runner = @'
param([string]$Helper, [string]$Config)
function global:Start-Sleep {
  param([int]$Seconds)
  Add-Content -LiteralPath $env:NEATI_CI_NSIS_DELAYS -Value $Seconds
}
& $Helper -Config $Config
# A nested script's exit sets LASTEXITCODE without terminating this wrapper.
# Match the GitHub Actions pwsh wrapper by forwarding that code explicitly.
exit $LASTEXITCODE
'@
  $runnerPath = Join-Path $fixtureRoot 'invoke-helper.ps1'
  [System.IO.File]::WriteAllText($runnerPath, $runner)
  $powershell = (Get-Process -Id $PID).Path
  $helper = Join-Path $PSScriptRoot 'build_ci_nsis.ps1'
  $cases = @(
    @{ Name = 'first success'; Scenario = 'success'; Exit = 0; Attempts = 1; Delays = ''; Config = '.github/tauri.package-ci.json' },
    @{ Name = 'ordinary compilation failure'; Scenario = 'compile-failure'; Exit = 23; Attempts = 1; Delays = ''; Config = '.github/tauri.package-ci.json' },
    @{ Name = 'unrelated download timeout'; Scenario = 'other-download-timeout'; Exit = 31; Attempts = 1; Delays = ''; Config = '.github/tauri.package-ci.json' },
    @{ Name = 'hash failure'; Scenario = 'hash-failure'; Exit = 41; Attempts = 1; Delays = ''; Config = '.github/tauri.package-ci.json' },
    @{ Name = 'bounded download timeout'; Scenario = 'timeout'; Exit = 31; Attempts = 3; Delays = '5,10'; Config = '.github/tauri.package-ci.json' },
    @{ Name = 'download retry recovery'; Scenario = 'timeout-then-success'; Exit = 0; Attempts = 3; Delays = '5,10'; Config = '.github/tauri.nsis-permachine.json' }
  )
  foreach ($case in $cases) {
    $env:PATH = $fixtureRoot
    $env:NEATI_CI_NSIS_CASE = $case.Scenario
    $env:NEATI_CI_NSIS_COUNT = Join-Path $fixtureRoot ($case.Scenario + '.count')
    $env:NEATI_CI_NSIS_ARGS = Join-Path $fixtureRoot ($case.Scenario + '.args')
    $env:NEATI_CI_NSIS_DELAYS = Join-Path $fixtureRoot ($case.Scenario + '.delays')
    $output = & $powershell -NoLogo -NoProfile -File $runnerPath -Helper $helper -Config $case.Config 2>&1
    $actualExit = $LASTEXITCODE
    $actualAttempts = [int](Get-Content -LiteralPath $env:NEATI_CI_NSIS_COUNT -Raw)
    if ($actualExit -ne $case.Exit -or $actualAttempts -ne $case.Attempts) {
      throw "$($case.Name): expected exit/attempts $($case.Exit)/$($case.Attempts), got $actualExit/$actualAttempts. $output"
    }
    $actualDelays = if (Test-Path -LiteralPath $env:NEATI_CI_NSIS_DELAYS) { (Get-Content -LiteralPath $env:NEATI_CI_NSIS_DELAYS) -join ',' } else { '' }
    if ($actualDelays -ne $case.Delays) {
      throw "$($case.Name): unexpected retry delays '$actualDelays'"
    }
    $expectedArguments = 'tauri build --debug --bundles nsis --config ' + $case.Config
    foreach ($arguments in Get-Content -LiteralPath $env:NEATI_CI_NSIS_ARGS) {
      if ($arguments -ne $expectedArguments) {
        throw "$($case.Name): forwarded unexpected arguments '$arguments'"
      }
    }
    Write-Host "PASS: $($case.Name)"
  }
} finally {
  $env:PATH = $originalPath
  foreach ($name in $fixtureVariables) {
    [Environment]::SetEnvironmentVariable($name, $originalVariables[$name])
  }
  Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
}
