param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [string]$BaselineInstaller
)
$ErrorActionPreference = 'Stop'

# Run on a disposable GitHub-hosted Windows runner, never against an active user's app.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'Installer smoke verification requires a disposable GitHub-hosted runner.'
}
if (Get-Process -Name DayNote -ErrorAction SilentlyContinue) { throw 'DayNote is already running.' }
$dataPath = Join-Path $env:LOCALAPPDATA 'DayNoteData'
$desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'DayNote.lnk'
if ((Test-Path -LiteralPath $dataPath) -or (Test-Path -LiteralPath $desktopShortcut) -or
    (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DayNote')) {
    throw 'Existing DayNote installation/data detected; refusing to overwrite it.'
}
$testRoot = Join-Path $env:RUNNER_TEMP ('daynote-installer-' + [guid]::NewGuid().ToString('N'))
$installPath = Join-Path $testRoot 'Program Files\DayNote'
$expectedVersion = (Get-Content -LiteralPath './package.json' -Raw | ConvertFrom-Json).version
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
$firstInstaller = if ($BaselineInstaller) { $BaselineInstaller } else { $Installer }
$setup = Start-Process -FilePath (Resolve-Path -LiteralPath $firstInstaller).Path -ArgumentList @('/S', "/D=$installPath") -WindowStyle Hidden -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
foreach ($file in @('DayNote.exe', 'daynote-cli.exe', 'LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'uninstall.exe')) {
    if (!(Test-Path -LiteralPath (Join-Path $installPath $file))) { throw "Missing installed file: $file" }
}
if (!(Test-Path -LiteralPath $desktopShortcut)) { throw 'Desktop shortcut was not created.' }
$shellLink = (New-Object -ComObject WScript.Shell).CreateShortcut($desktopShortcut)
if ($shellLink.TargetPath -ne (Join-Path $installPath 'DayNote.exe')) { throw "Shortcut points to the wrong executable: $($shellLink.TargetPath)" }
if ($shellLink.IconLocation -notlike "$(Join-Path $installPath 'DayNote.exe')*") { throw 'Shortcut does not use the application icon.' }
$cli = Join-Path $installPath 'daynote-cli.exe'
$added = & $cli add 'Installer verification task' | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Installed CLI failed to add a task.' }
$listed = & $cli list | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or @($listed.tasks).Count -ne 1) { throw 'Installed CLI could not read its task.' }
if (!(Test-Path -LiteralPath (Join-Path $dataPath 'daynote.db'))) { throw 'CLI did not use the user data directory.' }

if ($BaselineInstaller) {
    # Match the updater's NSIS /UPDATE mode. The registry must preserve the custom
    # installation path, including spaces, and the existing synthetic task.
    $upgraded = Start-Process -FilePath (Resolve-Path -LiteralPath $Installer).Path -ArgumentList @('/S', '/UPDATE') -WindowStyle Hidden -Wait -PassThru
    if ($upgraded.ExitCode -ne 0) { throw "Upgrade failed: $($upgraded.ExitCode)" }
    $upgradedTasks = & $cli list | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or @($upgradedTasks.tasks).Count -ne 1 -or $upgradedTasks.tasks[0].title -ne 'Installer verification task') {
        throw 'Upgrade did not preserve the existing task or install path.'
    }
    $upgradedShortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($desktopShortcut)
    if ($upgradedShortcut.TargetPath -ne (Join-Path $installPath 'DayNote.exe')) { throw 'Upgrade broke the desktop shortcut.' }
}

$inputPath = Join-Path $testRoot 'mcp-input.jsonl'
$outputPath = Join-Path $testRoot 'mcp-output.jsonl'
@'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"installer-smoke","version":"1"}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
'@ | Set-Content -LiteralPath $inputPath -Encoding utf8
$mcp = Start-Process -FilePath $cli -ArgumentList 'mcp' -WindowStyle Hidden -RedirectStandardInput $inputPath -RedirectStandardOutput $outputPath -RedirectStandardError (Join-Path $testRoot 'mcp-stderr.txt') -Wait -PassThru
if ($mcp.ExitCode -ne 0) { throw 'Installed MCP process failed.' }
$responses = @(Get-Content -LiteralPath $outputPath | ForEach-Object { $_ | ConvertFrom-Json })
if ($responses.Count -ne 2 -or !$responses[0].result.serverInfo -or !$responses[1].result.tools) { throw 'Installed MCP handshake/tools response is invalid.' }

$app = Start-Process -FilePath (Join-Path $installPath 'DayNote.exe') -WindowStyle Hidden -PassThru
try {
    $healthy = $false
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if ($app.HasExited) { throw "Installed desktop app exited: $($app.ExitCode)" }
        try {
            $health = Invoke-RestMethod 'http://127.0.0.1:47821/v1/health' -TimeoutSec 1
            if ($health.ok -and $health.version -eq $expectedVersion) { $healthy = $true; break }
        } catch { }
        Start-Sleep -Milliseconds 500
    }
    if (!$healthy) { throw 'Installed desktop app did not expose a healthy HTTP service.' }
    $token = (Get-Content -LiteralPath (Join-Path $dataPath 'api-token.txt') -Raw).Trim()
    $desktopTasks = Invoke-RestMethod 'http://127.0.0.1:47821/v1/tasks' -Headers @{Authorization = "Bearer $token"}
    if (@($desktopTasks.tasks).Count -ne 1) { throw 'Desktop HTTP and installed CLI do not share task data.' }
    node ./scripts/verify-shared-mcp.mjs 'http://127.0.0.1:47821/mcp' (Join-Path $dataPath 'api-token.txt') $expectedVersion --write-fixture
    if ($LASTEXITCODE -ne 0) { throw 'Installed shared HTTP MCP verification failed.' }
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort 47821)
    if ($listeners.Count -ne 1 -or $listeners[0].OwningProcess -ne $app.Id) { throw 'Shared MCP must use the single desktop listener.' }
    if (Get-Process -Name daynote-cli -ErrorAction SilentlyContinue) { throw 'Shared MCP left a CLI server running.' }
    $token = $null
} finally {
    # Only stop the process this isolated smoke test started; all data is synthetic.
    if (!$app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit() }
}

$uninstall = Join-Path $installPath 'uninstall.exe'
# _?= runs in-place so waiting covers the uninstaller rather than its launcher.
$removed = Start-Process -FilePath $uninstall -ArgumentList @('/S', "_?=$installPath") -WindowStyle Hidden -Wait -PassThru
if ($removed.ExitCode -ne 0) { throw "Uninstaller failed: $($removed.ExitCode)" }
if (Test-Path -LiteralPath (Join-Path $installPath 'DayNote.exe')) { throw 'Uninstall left the application executable.' }
if (Test-Path -LiteralPath $desktopShortcut) { throw 'Uninstall left the desktop shortcut.' }
if (!(Test-Path -LiteralPath (Join-Path $dataPath 'daynote.db'))) { throw 'Uninstall removed user task data.' }
$retained = & './target/release/daynote-cli.exe' list | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or @($retained.tasks).Count -ne 1) { throw 'Uninstall did not preserve readable task data.' }
Write-Output 'PASS: installer files, desktop target/icon, legacy CLI/MCP, 20 shared HTTP MCP clients, single desktop listener, zero resident CLI processes, uninstall, retained task database.'
if ($BaselineInstaller) { Write-Output 'PASS: in-place /UPDATE from baseline retained install path, desktop shortcut and task data.' }
