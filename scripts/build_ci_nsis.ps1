<#
Retry only Tauri's transient NSIS tool-download timeout. The bundler still owns
tool downloads, hash verification, extraction and installer construction.
#>
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet('.github/tauri.package-ci.json', '.github/tauri.nsis-permachine.json')]
  [string]$Config
)

$ErrorActionPreference = 'Stop'
# Capture native failures below instead of turning a nonzero exit into a
# terminating PowerShell error before its packaging log can be inspected.
$PSNativeCommandUseErrorActionPreference = $false

for ($attempt = 1; $attempt -le 3; $attempt++) {
  & pnpm tauri build --debug --bundles nsis --config $Config 2>&1 |
    Tee-Object -Variable buildOutput
  $buildExit = $LASTEXITCODE
  if ($buildExit -eq 0) {
    exit 0
  }

  $log = $buildOutput -join "`n"
  $toolDownloadTimeout = $log -match '(?s)Downloading https://github\.com/tauri-apps/(?:binary-releases|nsis-tauri-utils)/releases/download/[^\r\n]+\s+[^\r\n]*failed to bundle project: `timeout: global`'
  if (-not $toolDownloadTimeout -or $attempt -eq 3) {
    exit $buildExit
  }

  Write-Warning "NSIS tool download timed out; retrying packaging ($attempt/3)."
  Start-Sleep -Seconds (5 * $attempt)
}
