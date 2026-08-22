[CmdletBinding()]
param(
    [string[]]$Project,
    [string]$RadPath = "target/release/rad.exe",
    [string]$OutputRoot = "artifacts/portfolio",
    [switch]$AllowDirty,
    [switch]$SkipRepositoryGates
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

function Quote-NativeArgument([string]$Value) {
    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + ($Value -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
}

function Get-SafeName([string]$Value) {
    return ($Value -replace '[^A-Za-z0-9_.-]', '_').Trim('_')
}

function Get-RepoRelativePath([string]$Value) {
    $full = [IO.Path]::GetFullPath($Value)
    $prefix = $repoRoot.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        return $full.Substring($prefix.Length)
    }
    return $full
}

$script:results = [System.Collections.ArrayList]::new()
$script:sequence = 0

function Invoke-AcceptanceProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Executable,
        [string[]]$Arguments = @(),
        [int[]]$ExpectedExitCodes = @(0),
        [string]$ExpectedPattern,
        [int]$TimeoutSeconds = 7200,
        [string]$Category = "portfolio",
        [string]$ProjectName
    )

    $script:sequence += 1
    $safe = Get-SafeName ("{0:D3}-{1}" -f $script:sequence, $Name)
    $stdoutPath = Join-Path $script:logDirectory "$safe.stdout.txt"
    $stderrPath = Join-Path $script:logDirectory "$safe.stderr.txt"
    $resolvedExecutable = if ([IO.Path]::IsPathRooted($Executable)) {
        $Executable
    } else {
        $candidate = Join-Path $repoRoot $Executable
        if (Test-Path -LiteralPath $candidate) { $candidate } else { $Executable }
    }

    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedExecutable
    $startInfo.Arguments = ($Arguments | ForEach-Object { Quote-NativeArgument $_ }) -join ' '
    $startInfo.WorkingDirectory = $repoRoot
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    Write-Host ("[{0:D3}] {1}" -f $script:sequence, $Name)
    $startedUtc = [DateTime]::UtcNow
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($startInfo)
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $peakWorkingSetBytes = [uint64]0
    $peakPrivateBytes = [uint64]0
    $peakPagedMemoryBytes = [uint64]0
    $cpuNs = [uint64]0
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not $process.WaitForExit(50)) {
        $process.Refresh()
        $peakWorkingSetBytes = [Math]::Max($peakWorkingSetBytes, [uint64]$process.WorkingSet64)
        $peakPrivateBytes = [Math]::Max($peakPrivateBytes, [uint64]$process.PrivateMemorySize64)
        $peakPagedMemoryBytes = [Math]::Max($peakPagedMemoryBytes, [uint64]$process.PagedMemorySize64)
        $cpuNs = [uint64]($process.TotalProcessorTime.TotalMilliseconds * 1000000)
        if ([DateTime]::UtcNow -ge $deadline) { break }
    }
    $timedOut = -not $process.HasExited
    if ($timedOut) {
        $process.Kill()
        $process.WaitForExit()
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $stopwatch.Stop()
    $process.Refresh()

    [IO.File]::WriteAllText($stdoutPath, $stdout, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($stderrPath, $stderr, [Text.UTF8Encoding]::new($false))
    $combined = $stdout + "`n" + $stderr
    $exitCode = if ($timedOut) { -1 } else { $process.ExitCode }
    $exitPass = $ExpectedExitCodes -contains $exitCode
    $patternPass = [string]::IsNullOrEmpty($ExpectedPattern) -or $combined -match $ExpectedPattern
    $passed = -not $timedOut -and $exitPass -and $patternPass

    $result = [ordered]@{
        sequence = $script:sequence
        name = $Name
        category = $Category
        project = $ProjectName
        command = $resolvedExecutable
        arguments = @($Arguments)
        startedUtc = $startedUtc.ToString('o')
        elapsedNs = [uint64]($stopwatch.Elapsed.TotalMilliseconds * 1000000)
        cpuNs = $cpuNs
        peakWorkingSetBytes = $peakWorkingSetBytes
        peakPrivateBytes = $peakPrivateBytes
        peakPagedMemoryBytes = $peakPagedMemoryBytes
        exitCode = $exitCode
        expectedExitCodes = @($ExpectedExitCodes)
        expectedPattern = $ExpectedPattern
        timedOut = $timedOut
        passed = $passed
        stdout = Get-RepoRelativePath $stdoutPath
        stderr = Get-RepoRelativePath $stderrPath
    }
    [void]$script:results.Add([pscustomobject]$result)
    if (-not $passed) {
        Write-Host "  FAIL exit=$exitCode pattern=$patternPass" -ForegroundColor Red
    }
    return [pscustomobject]@{
        Result = [pscustomobject]$result
        Stdout = $stdout
        Stderr = $stderr
        Combined = $combined
    }
}

function Rad-Step {
    param(
        [string]$Name,
        [string[]]$Arguments,
        [int[]]$ExpectedExitCodes = @(0),
        [string]$ExpectedPattern,
        [string]$Category = "portfolio",
        [string]$ProjectName,
        [int]$TimeoutSeconds = 7200
    )
    return Invoke-AcceptanceProcess -Name $Name -Executable $script:resolvedRad `
        -Arguments $Arguments -ExpectedExitCodes $ExpectedExitCodes `
        -ExpectedPattern $ExpectedPattern -Category $Category `
        -ProjectName $ProjectName -TimeoutSeconds $TimeoutSeconds
}

function Negative([string]$File, [string]$Pattern) {
    return [ordered]@{ file = $File; pattern = $Pattern; mode = 'run' }
}

function Model-Negative([string]$File, [string]$Pattern) {
    return [ordered]@{ file = $File; pattern = $Pattern; mode = 'model' }
}

$projects = @(
    [ordered]@{
        name='pagergrid'; score=18; effect='RouteCritical'; path=@('RouteCritical','append_timeline'); plan='RouteCritical'; why=@('why','incident_0','IncidentTimeline')
        negatives=@(
            (Negative 'callback_io.rad' 'performs IO'),
            (Negative 'flush_handler.rad' 'writes \[IncidentTimeline\]'),
            (Negative 'hidden_write.rad' 'writes \[IncidentTimeline\]')
        )
    },
    [ordered]@{
        name='fulfillos'; score=18; effect='ReserveOrders'; path=@('ReserveOrders','reserve_stock'); plan='ReserveOrders'; why=@('why','0','Inventory')
        negatives=@(
            (Negative 'shipping_direct_write.rad' 'Ownership violation.*Inventory'),
            (Negative 'concealed_replacement.rad' 'Ownership violation.*Inventory'),
            (Negative 'owned_field.rad' 'Ownership violation.*ProductProfile\.reorder_level')
        )
    },
    [ordered]@{
        name='clearpay'; score=18; effect='CapturePayment'; path=@('CapturePayment','ledger_is_balanced'); plan='CapturePayment'; why=@('why','0','AccountBalance')
        negatives=@(
            (Negative 'imbalanced_ledger.rad' 'failed postcondition ensures #1'),
            (Negative 'hidden_changes_only.rad' 'writes outside changes_only \[MerchantRiskProfile\]'),
            (Negative 'post_commit_write.rad' 'post_commit Capture.*writes authoritative state \[Balance\]')
        )
    },
    [ordered]@{
        name='forgelink'; score=19; effect='StoreFrame'; path=@('ingest','StoreFrame'); plan='ingest'; why=@('why','0','DeviceTelemetry')
        negatives=@(
            (Negative 'wrong_nominal_id.rad' 'expects DeviceId, got SessionId'),
            (Negative 'wrong_endian.rad' 'endianness mismatch'),
            (Negative 'overlapping_flags.rad' 'Overlapping mask'),
            (Negative 'integer_overflow.rad' 'conversion overflow for u8')
        )
    },
    [ordered]@{
        name='marketlens'; score=19; effect='InspectSellable'; path=@('run_catalog_day','SetInventory'); plan='InspectSellable'; why=@('why-not-in-view','SellableProducts','0')
        negatives=@(
            (Negative 'hidden_dependency.rad' 'reads Inventory.*absent from depends'),
            (Negative 'unknown_predicate_field.rad' 'Unknown field.*Inventory\.on_hand'),
            (Negative 'reader_rebuild.rad' 'violates its no-full-scan contract'),
            (Negative 'manual_view_mutation.rad' 'runtime-maintained and cannot be mutated directly')
        )
    },
    [ordered]@{
        name='bookcore'; score=19; effect='MatchOrders'; path=@('MatchOrders','MatchBook'); plan='MatchOrders'; why=@('why','0','BuyOrder')
        negatives=@(
            (Negative 'sort_every_cycle.rad' 'violates its no-full-scan contract'),
            (Negative 'signed_as_unsigned.rad' 'expects PriceTicks, got UnsignedPrice'),
            (Negative 'duplicate_priority_bypass.rad' 'Ownership violation.*OrderIdentity'),
            (Negative 'amend_without_owner.rad' 'Ownership violation.*BuyOrder')
        )
    },
    [ordered]@{
        name='dispatch60'; score=19; effect='AdvanceActiveTrips'; path=@('AdvanceActiveTrips','advance_trip'); plan='AdvanceActiveTrips'; why=@('why-field','0','TripPosition','x')
        negatives=@(
            (Negative 'hidden_scan.rad' 'violates its no-full-scan contract'),
            (Negative 'hidden_allocation.rad' 'violates its no-guest-allocation contract'),
            (Negative 'dynamic_field.rad' 'requires a string-literal field name'),
            (Negative 'direct_owned_write.rad' 'Ownership violation.*TripPosition\.x')
        )
    },
    [ordered]@{
        name='matchflow'; score=18; effect='execute_authoritative_frame'; path=@('execute_authoritative_frame','ApplyDamage'); plan='AlivePlayers'; why=@('why','0','OutboundSnapshot')
        negatives=@(
            (Negative 'nested_flush.rad' 'flush_events\(\) is forbidden.*no_nested_flush'),
            (Negative 'late_phase_emit.rad' 'phase.*Resolve.*already completed'),
            (Negative 'duplicate_exactly_once.rad' 'Exactly-once handler.*duplicate delivery'),
            (Negative 'completion_barrier.rad' 'must complete before lifecycle phase.*Commit')
        )
    },
    [ordered]@{
        name='accesslens'; score=18; effect='expire_contractor'; path=@('expire_contractor','RevokeProductionAccess'); plan='ProductionDeployers'; why=@('why-removed','0','Entitlement')
        negatives=@(
            (Negative 'wrong_revision_expectation.rad' 'incorrectly expected unrelated text'),
            (Negative 'manual_view_mutation.rad' 'runtime-maintained and cannot be mutated directly'),
            (Negative 'direct_trust_mutation.rad' 'Ownership violation.*DeviceTrust'),
            (Negative 'direct_entitlement_remove.rad' 'Ownership violation.*Entitlement')
        )
    },
    [ordered]@{
        name='workpulse'; score=20; effect='InspectRetryQueue'; path=@('InspectRetryQueue','inspect_retry'); plan='RetryQueue'; why=@('why-removed','1','ActiveLease')
        negatives=@(
            (Model-Negative '01_stale_completion.rad' 'RejectStaleCompletion'),
            (Model-Negative '02_double_lease.rad' 'OneActiveLease'),
            (Model-Negative '03_timeout_retains_lease.rad' 'TimeoutClearsLease'),
            (Model-Negative '04_requeue_succeeded.rad' 'TerminalNeverRequeues'),
            (Model-Negative '05_retry_limit_order.rad' 'RetryCountBound'),
            (Model-Negative '06_duplicate_completion.rad' 'CompletionExactlyOnce'),
            (Model-Negative '07_restart_loses_queue.rad' 'RestartPreservesAcceptedJobs'),
            (Model-Negative '08_cancel_complete_race.rad' 'OneTerminalOutcome'),
            (Model-Negative '09_exhausted_not_dead_lettered.rad' 'ExhaustionDeadLetters'),
            (Model-Negative '10_event_before_commit.rad' 'PostCommitDeliveryOrder')
        )
    },
    [ordered]@{
        name='riskbridge'; score=20; effect='adjudicate_native'; path=@('adjudicate_native','score_native'); plan='InspectManualReviewQueue'; why=@('why','2','PolicyDecision')
        negatives=@(
            (Negative 'unknown_reason_flags.rad' 'unknown reason flags'),
            (Negative 'unknown_disposition.rad' 'disposition is unknown'),
            (Negative 'snapshot_host_handle.rad' 'cannot encode host_handle'),
            (Negative 'plugin_timeout.rad' 'HostCallFailure.*exceeded.*timeout'),
            (Negative 'plugin_crash.rad' 'HostCallFailure.*worker failed'),
            (Negative 'opaque_identity.rad' 'expects TransactionId, got CustomerId'),
            (Negative 'nonfinite_score.rad' 'score must be finite'),
            (Negative 'malformed_output.rad' 'RiskOutput transport is valid JSON'),
            (Negative 'host_success_failed_contract.rad' 'failed postcondition ensures #1'),
            (Negative 'host_failure.rad' 'HostCallFailure.*typed fixture host failure'),
            (Negative 'generation_change_in_transaction.rad' 'performs IO before commit')
        )
    }
)

$selectedProjects = if ($Project.Count -gt 0) {
    @($projects | Where-Object { $Project -contains $_.name })
} else {
    $projects
}
if ($selectedProjects.Count -eq 0) {
    throw "No project matched: $($Project -join ', ')"
}

$status = & git.exe status --porcelain=v1
if ($LASTEXITCODE -ne 0) { throw 'git status failed' }
$dirty = -not [string]::IsNullOrWhiteSpace(($status -join "`n"))
if ($dirty -and -not $AllowDirty) {
    throw 'Release acceptance requires a clean source generation. Commit first, or use -AllowDirty for a non-release diagnostic run.'
}

$commit = (& git.exe rev-parse HEAD).Trim()
$tree = if ($dirty) { 'dirty-working-tree' } else { (& git.exe rev-parse 'HEAD^{tree}').Trim() }
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
$runName = "$stamp-$($commit.Substring(0, 12))"
$script:outputDirectory = Join-Path (Join-Path $repoRoot $OutputRoot) $runName
$script:logDirectory = Join-Path $script:outputDirectory 'logs'
$traceDirectory = Join-Path $script:outputDirectory 'replays'
$modelArtifactDirectory = Join-Path $script:outputDirectory 'model-failures'
New-Item -ItemType Directory -Force -Path $script:logDirectory, $traceDirectory, $modelArtifactDirectory | Out-Null

if (-not $SkipRepositoryGates) {
    Invoke-AcceptanceProcess -Name 'build-release-workspace' -Executable 'cargo.exe' `
        -Arguments @('build','--workspace','--release','-j','1') -Category 'build' | Out-Null
}
$script:resolvedRad = (Resolve-Path (Join-Path $repoRoot $RadPath)).Path

if ($selectedProjects.name -contains 'riskbridge') {
    Invoke-AcceptanceProcess -Name 'build-riskbridge-plugin' -Executable 'cargo.exe' `
        -Arguments @('build','--manifest-path','projects/dogfood/riskbridge/plugins/risk-model/Cargo.toml','--target-dir','target/riskbridge-plugin','--release','-j','1') -Category 'build' -ProjectName 'riskbridge' | Out-Null
    Invoke-AcceptanceProcess -Name 'build-riskbridge-fixtures' -Executable 'powershell.exe' `
        -Arguments @('-NoProfile','-ExecutionPolicy','Bypass','-File','devtools/build-riskbridge-fixtures.ps1') -Category 'build' -ProjectName 'riskbridge' | Out-Null
}

$projectReports = [System.Collections.ArrayList]::new()
foreach ($definition in $selectedProjects) {
    $name = $definition.name
    $root = "projects/dogfood/$name"
    $before = $script:results.Count
    Rad-Step "$name-main" @("$root/main.rad") -ProjectName $name | Out-Null
    Rad-Step "$name-workflow-tests" @('test',"$root/tests") -ProjectName $name | Out-Null

    foreach ($negative in $definition.negatives) {
        $negativePath = "$root/negative/$($negative.file)"
        if ($negative.mode -eq 'model') {
            $outcome = Rad-Step "$name-negative-$($negative.file)" @(
                'model-check',$negativePath,'--runs','1','--max-commands','20','--seed','1234',
                '--artifact-dir',$modelArtifactDirectory,'--json'
            ) -ExpectedExitCodes @(1) -ExpectedPattern $negative.pattern -ProjectName $name -Category 'negative'
            $artifact = Get-ChildItem -LiteralPath $modelArtifactDirectory -Filter '*.radr' |
                Where-Object { $_.Name.StartsWith([IO.Path]::GetFileNameWithoutExtension($negative.file) + '.') } |
                Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
            if ($null -ne $artifact) {
                Rad-Step "$name-replay-$($negative.file)" @('replay',$artifact.FullName) `
                    -ExpectedExitCodes @(1) -ExpectedPattern 'Model failure reproduced' -ProjectName $name -Category 'negative-replay' | Out-Null
                Rad-Step "$name-shrink-$($negative.file)" @('shrink',$artifact.FullName) `
                    -ExpectedPattern 'deterministically shrunk.*to 1' -ProjectName $name -Category 'negative-shrink' | Out-Null
            }
        } else {
            Rad-Step "$name-negative-$($negative.file)" @($negativePath) `
                -ExpectedExitCodes @(1) -ExpectedPattern $negative.pattern -ProjectName $name -Category 'negative' | Out-Null
        }
    }

    $trace = Join-Path $traceDirectory "$name.radr"
    Rad-Step "$name-record" @("$root/main.rad",'--record',$trace) -ProjectName $name -Category 'replay' | Out-Null
    Rad-Step "$name-replay" @('replay',$trace) -ExpectedPattern 'Replay verified: world digest matches' -ProjectName $name -Category 'replay' | Out-Null
    Rad-Step "$name-effects" @('effects',$definition.effect,'--file',"$root/main.rad",'--json') -ProjectName $name -Category 'inspection' | Out-Null
    Rad-Step "$name-path" @('path',$definition.path[0],'->',$definition.path[1],'--file',"$root/main.rad",'--json') `
        -ExpectedPattern '"path"\s*:\s*\[' -ProjectName $name -Category 'inspection' | Out-Null
    Rad-Step "$name-query-plan" @('query-plan',$definition.plan,'--file',"$root/main.rad",'--json') -ProjectName $name -Category 'inspection' | Out-Null
    Rad-Step "$name-why" @($definition.why + @('--file',"$root/main.rad",'--json')) -ProjectName $name -Category 'inspection' | Out-Null

    $benchArguments = @('bench',"$root/bench.rad",'--json')
    if ($name -eq 'riskbridge') {
        $benchArguments += @('--','1000000','1000','native','none')
    }
    Rad-Step "$name-benchmark" $benchArguments -ProjectName $name -Category 'benchmark' -TimeoutSeconds 14400 | Out-Null

    if ($name -eq 'workpulse') {
        Rad-Step 'workpulse-model-campaign' @(
            'model-check',"$root/tests/job_model.rad",'--model','JobLifecycleModel',
            '--runs','10000','--max-commands','200','--seed','1234',
            '--artifact-dir',$modelArtifactDirectory,'--json'
        ) -ProjectName $name -Category 'model-campaign' -TimeoutSeconds 14400 | Out-Null
    }

    if ($name -eq 'riskbridge') {
        $validPlugin = 'target/riskbridge-plugin/release/riskbridge_model_plugin.dll'
        $contract = 'projects/dogfood/riskbridge/ffi-contract.json'
        Rad-Step 'riskbridge-ffi-valid' @('ffi','verify',$validPlugin,'--contract',$contract,'--json') `
            -ExpectedPattern '"compatible"\s*:\s*true' -ProjectName $name -Category 'ffi' | Out-Null
        $ffiNegatives = [ordered]@{
            wrong_abi='ABI mismatch'; wrong_calling_convention='calling convention'; wrong_struct_size='size/alignment mismatch';
            wrong_field_offset='field.*layout mismatch'; swapped_opaque_type='opaque type mismatch'; overlapping_layout='overlaps or exceeds';
            declared_io='effect/replay contract mismatch'; nondeterministic='determinism probe failed'; missing_export='export count mismatch'
        }
        foreach ($fixture in $ffiNegatives.GetEnumerator()) {
            Rad-Step "riskbridge-ffi-negative-$($fixture.Key)" @(
                'ffi','verify',"target/riskbridge-fixtures/$($fixture.Key).dll",'--contract',$contract
            ) -ExpectedExitCodes @(1) -ExpectedPattern $fixture.Value -ProjectName $name -Category 'ffi-negative' | Out-Null
        }
        $riskScaleTrace = Join-Path $traceDirectory 'riskbridge-million.radr'
        Rad-Step 'riskbridge-million-record' @(
            "$root/bench.rad",'--record',$riskScaleTrace,'--','1000000','1000','native','none'
        ) -ProjectName $name -Category 'benchmark-replay' -TimeoutSeconds 14400 | Out-Null
        Rad-Step 'riskbridge-million-replay' @('replay',$riskScaleTrace) `
            -ExpectedPattern 'Replay verified: world digest matches' -ProjectName $name -Category 'benchmark-replay' -TimeoutSeconds 14400 | Out-Null
        Rad-Step 'riskbridge-million-reference' @(
            'bench',"$root/bench.rad",'--json','--','1000000','1000','reference','none'
        ) -ProjectName $name -Category 'benchmark-reference' -TimeoutSeconds 14400 | Out-Null
    }

    $projectResults = @($script:results | Select-Object -Skip $before)
    [void]$projectReports.Add([pscustomobject][ordered]@{
        project = $name
        score = $definition.score
        passed = @($projectResults | Where-Object { -not $_.passed }).Count -eq 0
        stepsPassed = @($projectResults | Where-Object passed).Count
        stepsTotal = $projectResults.Count
    })
}

if (-not $SkipRepositoryGates) {
    $gates = @(
        @('format', 'cargo.exe', @('fmt','--all','--','--check')),
        @('workspace-check', 'cargo.exe', @('check','--workspace','--all-targets','-j','1')),
        @('vm-suite', 'cargo.exe', @('test','-p','rad-vm','-j','1')),
        @('cli-suite', 'cargo.exe', @('test','-p','rad-cli','-j','1')),
        @('lsp-suite', 'cargo.exe', @('test','-p','rad-lsp','-j','1')),
        @('strict-clippy', 'cargo.exe', @('clippy','--workspace','--all-targets','-j','1','--','-D','warnings')),
        @('architecture-tests', 'python.exe', @('-m','unittest','tooling.test_check_architecture','tooling.test_check_line_limits')),
        @('architecture-gate', 'python.exe', @('tooling/check_architecture.py')),
        @('line-limit-gate', 'python.exe', @('tooling/check_line_limits.py')),
        @('conformance-snapshots', $script:resolvedRad, @('snapshot','tests/conformance')),
        @('feature-snapshots', $script:resolvedRad, @('snapshot','tests/features')),
        @('causal-law-snapshots', $script:resolvedRad, @('snapshot','--experimental-laws','tests/fixtures/causal-laws')),
        @('candidate-constraint-snapshots', $script:resolvedRad, @('snapshot','--experimental-laws','tests/fixtures/causal-constraints')),
        @('documentation-links', 'python.exe', @('tooling/scripts/check_doc_links.py')),
        @('documentation-build', 'mdbook.exe', @('build','docs')),
        @('wasm-check', 'cargo.exe', @('check','-p','rad-vm','--lib','--target','wasm32-unknown-unknown','-j','1'))
    )
    foreach ($gate in $gates) {
        Invoke-AcceptanceProcess -Name $gate[0] -Executable $gate[1] -Arguments $gate[2] -Category 'repository-gate' -TimeoutSeconds 14400 | Out-Null
    }
}

$binaryHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $script:resolvedRad).Hash.ToLowerInvariant()
$lockHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $repoRoot 'Cargo.lock')).Hash.ToLowerInvariant()
$rustcVersion = (& rustc.exe -vV) -join "`n"
$allPassed = @($script:results | Where-Object { -not $_.passed }).Count -eq 0
$report = [ordered]@{
    kind = 'rad_portfolio_acceptance_v1'
    releaseEligible = (-not $dirty) -and (-not $SkipRepositoryGates) -and $selectedProjects.Count -eq 11
    passed = $allPassed
    generatedUtc = [DateTime]::UtcNow.ToString('o')
    source = [ordered]@{
        commit = $commit
        tree = $tree
        dirty = $dirty
        cargoLockSha256 = $lockHash
    }
    binary = [ordered]@{
        path = Get-RepoRelativePath $script:resolvedRad
        sha256 = $binaryHash
        buildProfile = 'release'
    }
    toolchain = $rustcVersion
    machine = [ordered]@{
        os = [Environment]::OSVersion.VersionString
        machine = [Environment]::MachineName
        processors = [Environment]::ProcessorCount
        powershell = $PSVersionTable.PSVersion.ToString()
    }
    projects = @($projectReports)
    results = @($script:results)
}
$reportPath = Join-Path $script:outputDirectory 'report.json'
[IO.File]::WriteAllText(
    $reportPath,
    ($report | ConvertTo-Json -Depth 20),
    [Text.UTF8Encoding]::new($false)
)
Write-Host "Portfolio report: $reportPath"
if (-not $allPassed) { exit 1 }
