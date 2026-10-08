param(
  [Parameter(Mandatory)][string]$Workspace,
  [Parameter(Mandatory)][string]$Identity,
  [Parameter(Mandatory)][string]$PromptFile,
  [string]$Room,
  [int]$MaxRunsPerDay = 60,
  [double]$MaxUsdPerRun = 5,
  [double]$MaxUsdPerDay = 50
)
$ErrorActionPreference = 'Stop'
$nodePath = (Get-Command node.exe).Source
$taskWorkspace = (Resolve-Path -LiteralPath $Workspace).Path
$taskPrompt = (Resolve-Path -LiteralPath $PromptFile).Path
$runnerPath = Join-Path $PSScriptRoot 'supervise.mjs'
$launchArgs = @($runnerPath, 'start', '--workspace', $taskWorkspace, '--identity', $Identity,
  '--prompt-file', $taskPrompt, '--max-runs-per-day', "$MaxRunsPerDay",
  '--max-usd-per-run', "$MaxUsdPerRun", '--max-usd-per-day', "$MaxUsdPerDay")
if ($Room) { $launchArgs += @('--room', $Room) }
& $nodePath @launchArgs
exit $LASTEXITCODE
