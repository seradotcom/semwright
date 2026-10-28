$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$etl = 'E:\Projects\semwright-blender52\.local\logs\cpu-resource-sxs-20261028T185809Z-0c04a049\cpu-resource-activation.etl'
$expected = '2db670e8a1ba8449bc7bf43841a76de7c7615edcfb08674b3214f542623ff6ba'
$needle = 'E:\Projects\semwright-blender52\.local\tmp\.tmp1wH4yv\'
$r = [ordered]@{status='NOT_READ'; source=$etl; event_limit=2048; new_trace=$false; cargo_repetitions=0; elevation_requested=$false; policy_changes=$false; attribution='Emitter PID/TID; client identity not inferred'; event_count=0; limit_reached=$false; payload_anchor_matches=0; payload_scan_incomplete=0; events=@()}
try {
    $r.sha256_before = (Get-FileHash -LiteralPath $etl -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($r.sha256_before -ne $expected) {throw 'SOURCE_ETL_HASH_MISMATCH'}
    $events = @(Get-WinEvent -Path $etl -Oldest -MaxEvents 2048 -ErrorAction Stop)
    $rows = [System.Collections.Generic.List[object]]::new()
    foreach ($e in $events) {
        $row = [ordered]@{id=$e.Id; version=$e.Version; provider=$e.ProviderName; provider_id=[string]$e.ProviderId; pid=$e.ProcessId; tid=$e.ThreadId; activity=[string]$e.ActivityId; related_activity=[string]$e.RelatedActivityId; utc=$null; payload_anchor=$false; payload_omitted=$true; scan_incomplete=$false}
        try {
            if ($null -ne $e.TimeCreated) {$row.utc=$e.TimeCreated.ToUniversalTime().ToString('o')}
            $xml = ''
            try {$xml=$e.ToXml()} catch {$row.xml_error=$_.Exception.Message; $row.scan_incomplete=$true}
            if ($xml.Length -gt 16384) {$xml=$xml.Substring(0,16384); $row.scan_incomplete=$true}
            $parts = [System.Collections.Generic.List[string]]::new();$parts.Add($xml)
            $properties = @()
            try {$properties=@($e.Properties)} catch {$row.properties_error=$_.Exception.Message; $row.scan_incomplete=$true}
            $row.properties_count=$properties.Count
            if ($properties.Count -gt 16) {$row.scan_incomplete=$true}
            foreach ($p in ($properties | Select-Object -First 16)) {
                $v=$p.Value
                if ($v -is [byte[]]) {
                    $n=[Math]::Min($v.Length,4096)
                    $parts.Add([Text.Encoding]::Unicode.GetString($v,0,$n))
                    $parts.Add([Text.Encoding]::UTF8.GetString($v,0,$n))
                    if ($v.Length -gt $n) {$row.scan_incomplete=$true}
                } else {
                    $text=[string]$v
                    if ($text.Length -gt 4096) {$text=$text.Substring(0,4096); $row.scan_incomplete=$true}
                    $parts.Add($text)
                }
            }
            $haystack=$parts -join [Environment]::NewLine
            $hit=$haystack.IndexOf($needle,[StringComparison]::OrdinalIgnoreCase)
            if ($hit -ge 0) {
                $row.payload_anchor=$true;$row.payload_omitted=$false;$r.payload_anchor_matches++
                $start=[Math]::Max(0,$hit-120);$length=[Math]::Min(2048,$haystack.Length-$start)
                $row.anchor_excerpt=$haystack.Substring($start,$length)
                $row.anchor_excerpt_truncated=($length -lt $haystack.Length-$start)
                $row.xml_prefix=$xml.Substring(0,[Math]::Min(4096,$xml.Length));$row.xml_prefix_truncated=($xml.Length -gt 4096)
            }
        } catch {$row.decode_error=$_.Exception.Message;$row.scan_incomplete=$true}
        finally {
            if ($row.scan_incomplete) {$r.payload_scan_incomplete++}
            $rows.Add([pscustomobject]$row);$e.Dispose()
        }
    }
    $r.event_count=$events.Count;$r.limit_reached=($events.Count -eq 2048);$r.events=$rows.ToArray()
    $r.status=if ($r.limit_reached) {'EVENT_LIMIT_REACHED_PARTIAL'} elseif ($r.payload_anchor_matches -eq 0) {'HEADERS_READ_NO_MATCHING_PAYLOAD'} else {'OWNED_ETL_METADATA_READ'}
} catch {
    $r.status=if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') {'NO_EVENTS_RETURNED_BY_GET_WINEVENT'} else {'READ_ERROR'}
    $r.error_id=$_.FullyQualifiedErrorId;$r.error_message=$_.Exception.Message;$r.hresult=$_.Exception.HResult
} finally {
    try {
        $r.sha256_after=(Get-FileHash -LiteralPath $etl -Algorithm SHA256).Hash.ToLowerInvariant()
        $r.source_unchanged=($r.sha256_after -eq $r.sha256_before)
        if (-not $r.source_unchanged) {$r.status='SOURCE_ETL_CHANGED_NOT_COMPARABLE'}
    } catch {$r.source_hash_after_error=$_.Exception.Message}
}
$r | ConvertTo-Json -Depth 10 -Compress
