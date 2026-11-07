<#
.SYNOPSIS
    Benchmark tmuxpanel the way a user runs it: inside a real psmux terminal.

.DESCRIPTION
    For each binary given, runs N rounds. Each round starts the TUI in a
    private psmux server (psmux -L bench-tmuxpanel), then measures:
      * startup_ms      launch to first frame on screen
      * tab_ms          Tab keystroke to the Browse screen being drawn
      * idle_cpu_ms     CPU time used while sitting idle for IdleSeconds
      * idle_ws_mb      working set after idling
      * peak_ws_mb      peak working set
    and, when the binary supports it, the CLI:
      * cli_list_ms     `tmuxpanel list -c <temp config>` wall time

    Raw rows go to CSV, plus a JSON summary with medians. Only the private
    server this script creates is ever killed.

.EXAMPLE
    .\scripts\bench.ps1 -Binaries @{ "v0.1.1" = "old\tmuxpanel.exe"; "v0.2.0" = "target\release\tmuxpanel.exe" }
#>
param(
    [Parameter(Mandatory)] [hashtable]$Binaries,
    [int]$Rounds = 10,
    [int]$IdleSeconds = 5,
    [string]$OutDir = (Join-Path $HOME ("psmux-bench\Tmux-Plugin-Panel\" + (Get-Date -Format yyyy-MM-dd)))
)

$ErrorActionPreference = 'Stop'
$server = "bench-tmuxpanel"
New-Item -ItemType Directory -Force $OutDir | Out-Null

function Wait-Screen([string]$pattern, [int]$timeoutMs = 15000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $screen = (psmux -L $server capture-pane -p -t bench 2>$null) -join "`n"
        if ($screen -match $pattern) { return $true }
        Start-Sleep -Milliseconds 10
    }
    return $false
}

function Stop-Bench {
    psmux -L $server kill-server 2>$null | Out-Null
}

# Temp config with its own plugin dir, so nothing in the real home is touched.
$cfgDir = Join-Path ([IO.Path]::GetTempPath()) "tmuxpanel-bench"
New-Item -ItemType Directory -Force $cfgDir | Out-Null
$cfg = Join-Path $cfgDir "tmux.conf"
"# bench`n# tmuxpanel:plugin-dir $cfgDir\plugins`nset -g @plugin 'tmux-plugins/tmux-sensible'`n" | Set-Content $cfg -NoNewline

$rows = @()
foreach ($label in $Binaries.Keys) {
    $exe = (Resolve-Path $Binaries[$label]).Path
    # Releases before 0.2.0 ignore arguments and open the TUI, so probe with
    # a timeout and kill the probe if it does not exit on its own.
    $probeOut = Join-Path ([IO.Path]::GetTempPath()) "tmuxpanel-bench-probe.txt"
    $probe = Start-Process $exe -ArgumentList "--version" -PassThru -NoNewWindow -RedirectStandardOutput $probeOut
    $hasCli = $probe.WaitForExit(3000) -and ((Get-Content $probeOut -Raw) -match "tmuxpanel")
    if (-not $probe.HasExited) { Stop-Process -Id $probe.Id -Force }
    Remove-Item $probeOut -ErrorAction SilentlyContinue
    for ($i = 1; $i -le $Rounds; $i++) {
        Stop-Bench
        $sw = [Diagnostics.Stopwatch]::StartNew()
        psmux -L $server new-session -d -s bench -x 160 -y 45 "`"$exe`""
        $ok = Wait-Screen "Tmux Plugin Panel"
        $startup = $sw.ElapsedMilliseconds
        if (-not $ok) { Write-Warning "$label round $i : no first frame"; Stop-Bench; continue }

        $proc = Get-Process | Where-Object { $_.Path -eq $exe } | Select-Object -First 1
        Start-Sleep -Milliseconds 300
        $sw.Restart()
        psmux -L $server send-keys -t bench Tab
        $null = Wait-Screen "CATEGORIES"
        $tab = $sw.ElapsedMilliseconds
        # back to Home so idle is measured on the same screen for every binary
        psmux -L $server send-keys -t bench BTab
        Start-Sleep -Milliseconds 300

        $proc.Refresh(); $cpu0 = $proc.TotalProcessorTime.TotalMilliseconds
        Start-Sleep -Seconds $IdleSeconds
        $proc.Refresh()
        $idleCpu = $proc.TotalProcessorTime.TotalMilliseconds - $cpu0
        $ws = [math]::Round($proc.WorkingSet64 / 1MB, 2)
        $peak = [math]::Round($proc.PeakWorkingSet64 / 1MB, 2)
        psmux -L $server send-keys -t bench q
        Start-Sleep -Milliseconds 300
        Stop-Bench

        $cli = ""
        if ($hasCli) {
            $sw.Restart(); & $exe list -c $cfg *> $null; $cli = $sw.ElapsedMilliseconds
        }
        $rows += [pscustomobject]@{
            version = $label; round = $i; startup_ms = $startup; tab_ms = $tab
            idle_cpu_ms = [math]::Round($idleCpu, 1); idle_ws_mb = $ws; peak_ws_mb = $peak; cli_list_ms = $cli
        }
        Write-Output ("{0} #{1}: startup {2} ms, tab {3} ms, idle cpu {4} ms, ws {5} MB" -f $label, $i, $startup, $tab, [math]::Round($idleCpu,1), $ws)
    }
}
Stop-Bench
Remove-Item -Recurse -Force $cfgDir -ErrorAction SilentlyContinue

function Median($xs) {
    $s = @($xs | Where-Object { $_ -ne "" } | ForEach-Object { [double]$_ } | Sort-Object)
    if ($s.Count -eq 0) { return $null }
    if ($s.Count % 2) { return $s[[int][math]::Floor($s.Count / 2)] }
    return ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2
}

$csv = Join-Path $OutDir "tmuxpanel-bench.csv"
$rows | Export-Csv $csv -NoTypeInformation
$summary = foreach ($label in $Binaries.Keys) {
    $r = $rows | Where-Object version -eq $label
    [ordered]@{
        version = $label; rounds = @($r).Count
        startup_ms = Median $r.startup_ms; tab_ms = Median $r.tab_ms
        idle_cpu_ms = Median $r.idle_cpu_ms; idle_ws_mb = Median $r.idle_ws_mb
        peak_ws_mb = Median $r.peak_ws_mb; cli_list_ms = Median $r.cli_list_ms
    }
}
$json = Join-Path $OutDir "tmuxpanel-bench-summary.json"
[ordered]@{
    machine = $env:COMPUTERNAME; date = (Get-Date -Format o); idle_seconds = $IdleSeconds
    note = "medians; startup/tab measured by polling psmux capture-pane every 10 ms"
    results = @($summary)
} | ConvertTo-Json -Depth 4 | Set-Content $json
Write-Output "raw: $csv"
Write-Output "summary: $json"
Get-Content $json
