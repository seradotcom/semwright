$ErrorActionPreference='Stop'
$root='E:\Projects\worldwright-archipelago'
$d=Join-Path $root 'evidence\integration\20261029-godot-broker\godot-bridge01'
$out=Join-Path $d 'operator-console02\secret-acl-recovery.json'
if(Test-Path -LiteralPath $out){throw 'Preserve previous ACL recovery receipt'}
$pin=Get-Content -Raw (Join-Path $d 'prepared.json')|ConvertFrom-Json
$secret=$pin.secret_file
if($secret -ne (Join-Path $root '.local\godot-bridge01-config\pairing.secret')){throw 'Secret path scope'}
$item=Get-Item -LiteralPath $secret
if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Secret cannot be a reparse point'}
$retired=Get-Content -Raw (Join-Path $d 'operator-console02\old-broker-retirement.json')|ConvertFrom-Json
if(!$retired.both_absent -or !$retired.editor_preserved){throw 'Prior broker not retired'}
$ids=@(37772,36556,43672)
if(@(Get-Process -Id $ids -ErrorAction SilentlyContinue).Count -ne 0){throw 'Retired process identity is still present'}
$editor=Get-Process -Id 39036 -ErrorAction Stop
if($editor.StartTime.ToUniversalTime().ToString('o') -ne '2026-10-29T02:43:31.6619060Z'){throw 'Owned Editor identity changed'}
$records=@(Get-Content (Join-Path $d 'broker.stderr.log') | ForEach-Object {$_|ConvertFrom-Json})
$attested=@($records | Where-Object phase -eq 'initial_process_attested_before_resume')
if($attested.Count -ne 1 -or $attested[0].pid -ne 36556){throw 'Host attestation mismatch'}
$acl=Get-Acl -LiteralPath $secret
$owner=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
if($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $owner){throw 'Owner mismatch'}
$before=$acl.Sddl
$secretHash=(Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash
$secretSize=$item.Length
$explicit=@($acl.GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier]))
if($explicit.Count -ne 2){throw 'Unexpected explicit ACL scope'}
$sid='S-1-15-2-1897327447-535692164-3318373662-1781345858-2826093919-4271363386-3537105231'
$sidObject=[Security.Principal.SecurityIdentifier]::new($sid)
$sidBytes=[byte[]]::new($sidObject.BinaryLength)
$sidObject.GetBinaryForm($sidBytes,0)
$sha=[Security.Cryptography.SHA256]::Create()
$matchedHash=([BitConverter]::ToString($sha.ComputeHash($sidBytes))).Replace('-','').ToLowerInvariant()
$sha.Dispose()
if($matchedHash -ne $attested[0].token.appcontainer_sid_sha256){throw 'SID does not match binary-SID Host attestation'}
$removed=@()
foreach($rule in $explicit){
    if($rule.IdentityReference.Value -ne $sid -or $rule.IsInherited -or [int]$rule.InheritanceFlags -ne 0 -or [int]$rule.PropagationFlags -ne 0){throw 'Unexpected ACL identity or flags'}
    $kind=$rule.AccessControlType.ToString()
    $mask=[int]$rule.FileSystemRights
    if(!(($kind -eq 'Deny' -and $mask -eq 852310) -or ($kind -eq 'Allow' -and $mask -eq 1179785))){throw 'Unexpected ACL permission mask'}
    $removed+=@{sid=$sid;type=$kind;mask=$mask;inheritance=0;propagation=0;inherited=$false}
    [void]$acl.RemoveAccessRuleSpecific($rule)
}
$expected=$acl.Sddl
if((Get-Acl -LiteralPath $secret).Sddl -ne $before -or (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash -ne $secretHash -or (Get-Item -LiteralPath $secret).Length -ne $secretSize){throw 'Concurrent secret or ACL change'}
# Remove only the two exact expired Host lease ACEs; add no permission and retain all other entries.
Set-Acl -LiteralPath $secret -AclObject $acl
$after=(Get-Acl -LiteralPath $secret).Sddl
$unchanged=(Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash -eq $secretHash -and (Get-Item -LiteralPath $secret).Length -eq $secretSize
$result=@{schema=1;utc=[DateTime]::UtcNow.ToString('o');secret_path=$secret;before_sddl=$before;after_sddl=$after;
removed_exact_rules=$removed;matched_host_sid_sha256=$matchedHash;sid_hash_representation='binary SID, same as initial_attestation.rs';
only_exact_lease_rules_removed=($after -eq $expected);secret_bytes_unchanged=$unchanged;secret_content_or_hash_recorded=$false;
new_permissions_added=$false;policy_changed=$false;owner_preserved=$true;editor_untouched=$true;retired_processes_absent=$ids;
private_validator_reexecution='PENDING';reason='Forced retirement left exact retired driver read lease on original pairing secret'}
[IO.File]::WriteAllText($out,($result|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
$result|ConvertTo-Json -Depth 8
if($after -ne $expected -or !$unchanged){throw 'Post-repair verification failed; inspect receipt without replay'}
