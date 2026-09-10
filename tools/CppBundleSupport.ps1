$script:CppBundleContractVersion = 'act/installable-cpp-bundle/v1'
$script:CppBundleProduct = 'ai-computer-toolkit'
$script:CppBundleMainExecutable = 'ai-computer-toolkit-cpp.exe'
$script:CppBundleExpectedArtifacts = [ordered]@{
    'ai-computer-toolkit-cpp.exe' = 'public-cli'
    'ai-computer-toolkit-observation-worker.exe' =
        'uia-observation-worker'
    'ai-computer-toolkit-capture-worker.exe' = 'capture-worker'
    'ai-computer-toolkit-media-worker.exe' =
        'media-observation-worker'
    'legacy-public-catalog-v1.json' =
        'descriptor-compatibility-manifest'
}

function Test-CppBundleVersion {
    param([object] $Version)

    return $Version -is [string] -and
        $Version -match '^[A-Za-z0-9][A-Za-z0-9._+-]{0,63}$'
}

function Test-CppPathWithin {
    param(
        [Parameter(Mandatory)]
        [string] $Candidate,
        [Parameter(Mandatory)]
        [string] $Root
    )

    $candidatePath = [System.IO.Path]::GetFullPath($Candidate)
    $rootPath = [System.IO.Path]::GetFullPath($Root)
    return $candidatePath -eq $rootPath -or
        $candidatePath.StartsWith(
            $rootPath + [System.IO.Path]::DirectorySeparatorChar,
            [System.StringComparison]::OrdinalIgnoreCase
        )
}

function Test-CppBundle {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $bundlePath = [System.IO.Path]::GetFullPath($Path)
    if (-not (Test-Path -LiteralPath $bundlePath -PathType Container)) {
        throw 'Bundle directory is unavailable.'
    }
    $manifestPath = Join-Path $bundlePath 'bundle-manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw 'Bundle manifest is missing.'
    }
    try {
        $manifest = Get-Content -LiteralPath $manifestPath -Raw |
            ConvertFrom-Json
    } catch {
        throw 'Bundle manifest is not valid JSON.'
    }
    $artifacts = @($manifest.artifacts)
    if ($manifest.contractVersion -ne
        $script:CppBundleContractVersion -or
        $manifest.product -ne $script:CppBundleProduct -or
        $manifest.mainExecutable -ne
        $script:CppBundleMainExecutable -or
        -not (Test-CppBundleVersion $manifest.bundleVersion) -or
        $artifacts.Count -ne
        $script:CppBundleExpectedArtifacts.Count) {
        throw 'Bundle manifest is invalid.'
    }

    $seen = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($artifact in $artifacts) {
        $artifactPath = [string] $artifact.path
        if (-not $script:CppBundleExpectedArtifacts.Contains(
            $artifactPath
        ) -or
            $artifactPath -ne
            [System.IO.Path]::GetFileName($artifactPath) -or
            -not $seen.Add($artifactPath)) {
            throw 'Bundle artifact path is unsafe or unexpected.'
        }
        $expectedRole =
            $script:CppBundleExpectedArtifacts[$artifactPath]
        if ($artifact.role -ne $expectedRole) {
            throw "Bundle artifact role is invalid: $artifactPath"
        }
        $file = Join-Path $bundlePath $artifactPath
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "Bundle artifact is missing: $artifactPath"
        }
        $item = Get-Item -LiteralPath $file
        $hash = (
            Get-FileHash -LiteralPath $file -Algorithm SHA256
        ).Hash
        if ($artifact.bytes -isnot [long] -and
            $artifact.bytes -isnot [int]) {
            throw "Bundle artifact size is invalid: $artifactPath"
        }
        if ([long] $artifact.bytes -le 0 -or
            $item.Length -ne [long] $artifact.bytes -or
            $artifact.sha256 -notmatch '^[0-9A-Fa-f]{64}$' -or
            -not $hash.Equals(
                [string] $artifact.sha256,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            throw "Bundle artifact verification failed: $artifactPath"
        }
    }

    $allowed = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    $null = $allowed.Add('bundle-manifest.json')
    foreach ($name in $script:CppBundleExpectedArtifacts.Keys) {
        $null = $allowed.Add($name)
    }
    foreach ($entry in Get-ChildItem -LiteralPath $bundlePath -Force) {
        if (-not $allowed.Contains($entry.Name) -or
            $entry.PSIsContainer) {
            throw "Bundle contains an unexpected entry: $($entry.Name)"
        }
    }
    return $manifest
}
