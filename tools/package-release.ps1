param(
    [string]$Version = "",
    [string]$Target = "",
    [switch]$SkipJavaBuild,
    [switch]$Archive
)

$ErrorActionPreference = "Stop"

function Default-Target {
    if (Test-IsWindows) { return "windows-x86_64" }
    if (Test-IsMacOS) { return Default-MacOS-Target }
    return "linux-x86_64"
}

function Default-MacOS-Target {
    if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq [System.Runtime.InteropServices.Architecture]::Arm64) {
        return "macos-aarch64"
    }
    throw "macOS release packaging supports Apple Silicon only. Run on an arm64 macOS runner or pass a supported target explicitly."
}

function Tui-Binary-Name {
    if (Test-IsWindows) { return "oprofiler-tui.exe" }
    return "oprofiler-tui"
}

function Native-Library-Name {
    if (Test-IsWindows) { return "jvmti_agent_rust.dll" }
    if (Test-IsMacOS) { return "libjvmti_agent_rust.dylib" }
    return "libjvmti_agent_rust.so"
}

function Test-IsWindows {
    return $env:OS -eq "Windows_NT"
}

function Test-IsMacOS {
    return ($PSVersionTable.PSEdition -eq "Core") -and $IsMacOS
}

function Read-Cargo-Version {
    param([string]$Path)

    $match = Get-Content $Path | Select-String -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($null -eq $match) {
        return ""
    }
    return $match.Matches.Groups[1].Value
}

if ([string]::IsNullOrWhiteSpace($Target)) {
    $Target = Default-Target
}

$repo = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Read-Cargo-Version (Join-Path $repo "crates/tui/Cargo.toml")
    if ([string]::IsNullOrWhiteSpace($Version)) {
        $Version = Read-Cargo-Version (Join-Path $repo "Cargo.toml")
    }
    if ([string]::IsNullOrWhiteSpace($Version)) {
        throw "Could not read package version from Cargo.toml"
    }
}
$distName = "openprofiler-$Version-$Target"
$dist = Join-Path $repo "dist/$distName"
$binaryName = Tui-Binary-Name
$nativeName = Native-Library-Name

Push-Location $repo
try {
    cargo build --release -p oprofiler-tui -p jvmti-agent-rust

    if (!$SkipJavaBuild) {
        mvn -q -f java-agent/pom.xml package
        javac --release 11 tools/AttachAgent.java
    }

    New-Item -ItemType Directory -Force $dist | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $dist "attach") | Out-Null

    $javaAgentJar = "java-agent/target/java-agent-$Version.jar"
    $attachAgentClass = "tools/AttachAgent.class"
    if (!(Test-Path $javaAgentJar)) {
        throw "Missing $javaAgentJar. Run without -SkipJavaBuild or download the Java release artifact before packaging."
    }
    if (!(Test-Path $attachAgentClass)) {
        throw "Missing $attachAgentClass. Run without -SkipJavaBuild or download the Java release artifact before packaging."
    }

    Copy-Item (Join-Path "target/release" $binaryName) (Join-Path $dist $binaryName) -Force
    Copy-Item $javaAgentJar (Join-Path $dist "java-agent-$Version.jar") -Force
    try {
        Copy-Item (Join-Path "target/release" $nativeName) (Join-Path $dist $nativeName) -Force
    } catch {
        if (Test-Path (Join-Path $dist $nativeName)) {
            Write-Warning "$nativeName is in use; keeping the existing packaged native library."
        } else {
            throw
        }
    }
    Copy-Item $attachAgentClass (Join-Path $dist "attach/AttachAgent.class") -Force

    if (!(Test-IsWindows)) {
        chmod +x (Join-Path $dist $binaryName)
    }

    if ($Archive) {
        if (Test-IsWindows) {
            $archivePath = Join-Path $repo "dist/$distName.zip"
            if (Test-Path $archivePath) { Remove-Item $archivePath -Force }
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [System.IO.Compression.ZipFile]::CreateFromDirectory(
                $dist,
                $archivePath,
                [System.IO.Compression.CompressionLevel]::Optimal,
                $false
            )
        } else {
            $archivePath = Join-Path $repo "dist/$distName.tar.gz"
            if (Test-Path $archivePath) { Remove-Item $archivePath -Force }
            tar -czf $archivePath -C (Join-Path $repo "dist") $distName
        }
        Write-Host "Archive: $archivePath"
    }

    Write-Host "Packaged $distName"
    Write-Host "GitHub release layout:"
    Write-Host "  $binaryName"
    Write-Host "  java-agent-$Version.jar"
    Write-Host "  $nativeName"
    Write-Host "  attach/AttachAgent.class"
} finally {
    Pop-Location
}
