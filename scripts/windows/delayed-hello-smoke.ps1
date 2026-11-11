# Real Windows named-pipe handshake using the built broker. No business commands.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$BinDir,
    [Parameter(Mandatory = $true)][string]$Output
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$BinDir = (Resolve-Path -LiteralPath $BinDir).Path
$Output = [IO.Path]::GetFullPath($Output)
if (-not $Output.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Test evidence must stay inside the checked-out source.'
}
if (Test-Path -LiteralPath $Output) { throw 'Test evidence already exists.' }
$cli = Join-Path $BinDir 'semwright.exe'
$daemon = Join-Path $BinDir 'semwrightd.exe'
foreach ($binary in @($cli, $daemon)) {
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Missing binary: $binary" }
}
New-Item -ItemType Directory -Path $Output -ErrorAction Stop | Out-Null
$env:SEMWRIGHT_DATA_DIR = Join-Path $Output 'data'
$socket = '\\.\pipe\semwright-hello-' + [Guid]::NewGuid().ToString('N')
$pipeName = $socket.Substring('\\.\pipe\'.Length)
$configDir = Join-Path $Output 'config'
$setupOutput = & $cli '--json' '--socket' $socket 'setup' '--config-dir' $configDir
if ($LASTEXITCODE -ne 0) { throw 'Owner setup failed.' }
$setup = $setupOutput | ConvertFrom-Json
if ($setup.setup -ne 'complete' -or $setup.authority_changed -ne $false -or $setup.service_started -ne $false) {
    throw 'Setup changed authority or started a service.'
}
$config = $setup.config.path
$daemonLog = Join-Path $Output 'broker.stdout.log'
$daemonErr = Join-Path $Output 'broker.stderr.log'
$daemonProcess = $null
function Read-Exact([System.IO.Stream]$Pipe, [int]$Size) {
    if ($Size -le 0 -or $Size -gt 1048576) { throw 'Invalid response frame length.' }
    $bytes = New-Object byte[] $Size
    $offset = 0
    while ($offset -lt $Size) {
        $stop = [System.Threading.CancellationTokenSource]::new(4000)
        try {
            $count = $Pipe.ReadAsync($bytes, $offset, $Size - $offset, $stop.Token).GetAwaiter().GetResult()
        } finally {
            $stop.Dispose()
        }
        if ($count -eq 0) { throw [System.IO.EndOfStreamException]::new('Pipe closed before response.') }
        $offset += $count
    }
    return ,$bytes
}
function Send-Hello([System.IO.Stream]$Pipe) {
    $body = [Text.Encoding]::UTF8.GetBytes('{"type":"hello","version":1,"session":null}')
    $header = [BitConverter]::GetBytes([Net.IPAddress]::HostToNetworkOrder([int]$body.Length))
    $Pipe.Write($header, 0, $header.Length)
    $Pipe.Write($body, 0, $body.Length)
    $Pipe.Flush()
}
function Read-Response([System.IO.Stream]$Pipe) {
    [byte[]]$header = Read-Exact $Pipe 4
    $size = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($header, 0))
    [byte[]]$body = Read-Exact $Pipe $size
    return [Text.Encoding]::UTF8.GetString($body) | ConvertFrom-Json
}
function Open-Client([System.Security.Principal.TokenImpersonationLevel]$Level) {
    $client = [System.IO.Pipes.NamedPipeClientStream]::new(
        '.', $pipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::Asynchronous,
        $Level
    )
    try {
        $client.Connect(10000)
        return $client
    } catch {
        $client.Dispose()
        throw
    }
}
$report = [ordered]@{
    source_sha = $env:GITHUB_SHA
    mode = 'native_windows_broker_pipe'
    fake = $false
    valid_hello_350ms = 'NOT_RUN'
    valid_hello_500ms = 'NOT_RUN'
    anonymous_impersonation = 'NOT_RUN'
    business_dispatches = 0
    cross_sid_tested = $false
    release_admission = $false
}
try {
    $daemonProcess = Start-Process -FilePath $daemon -ArgumentList @('--config', $config, '--socket', $socket) -PassThru -NoNewWindow -RedirectStandardOutput $daemonLog -RedirectStandardError $daemonErr
    [void]$daemonProcess.Handle
    foreach ($delay in @(350, 500)) {
        if ($daemonProcess.HasExited) { throw 'Broker exited before delayed Hello.' }
        $client = Open-Client ([System.Security.Principal.TokenImpersonationLevel]::Identification)
        try {
            Start-Sleep -Milliseconds $delay
            Send-Hello $client
            $reply = Read-Response $client
            if ($reply.type -ne 'welcome' -or $reply.version -ne 1 -or $reply.session -cnotmatch '^[0-9a-f]{32}$') {
                throw 'Delayed valid Hello was not admitted by the authenticated broker.'
            }
            if ($delay -eq 350) { $report.valid_hello_350ms = 'PASS' }
            else { $report.valid_hello_500ms = 'PASS' }
        } finally {
            $client.Dispose()
        }
    }
    if ($daemonProcess.HasExited) { throw 'Broker exited before negative peer test.' }
    # Anonymous SQOS cannot supply the owner's impersonation token.
    $client = Open-Client ([System.Security.Principal.TokenImpersonationLevel]::Anonymous)
    try {
        Start-Sleep -Milliseconds 350
        try {
            Send-Hello $client
            $reply = Read-Response $client
            throw "Anonymous peer received a broker response: $($reply.type)"
        } catch [System.IO.EndOfStreamException] {
            $report.anonymous_impersonation = 'DENIED'
        } catch [System.IO.IOException] {
            $report.anonymous_impersonation = 'DENIED'
        }
    } finally {
        $client.Dispose()
    }
    if ($report.anonymous_impersonation -ne 'DENIED') { throw 'Anonymous peer not rejected.' }
    Write-Output ($report | ConvertTo-Json -Depth 4)
} finally {
    if ($null -ne $daemonProcess) {
        if (-not $daemonProcess.HasExited) {
            Stop-Process -Id $daemonProcess.Id -Force -ErrorAction Stop
            [void]$daemonProcess.WaitForExit(5000)
        }
        $daemonProcess.Dispose()
    }
    ($report | ConvertTo-Json -Depth 4) | Set-Content -LiteralPath (Join-Path $Output 'handshake.json') -Encoding utf8
}
