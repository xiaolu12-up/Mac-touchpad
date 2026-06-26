param (
    [Parameter(Mandatory=$true)]
    [string]$Version
)

# Strip leading 'v' or 'V' if present to support both v0.1.2 and 0.1.2 formats
if ($Version.StartsWith("v") -or $Version.StartsWith("V")) {
    $Version = $Version.Substring(1)
}

# Validate version format (e.g. 0.1.2)
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Error "Version format error. Should be X.Y.Z (e.g. 0.1.1)"
    exit 1
}

Write-Host "Bumping version to: $Version ..."

# Helper function to write UTF-8 without BOM
function Write-Utf8NoBom ($FilePath, $Content) {
    $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $AbsolutePath = [System.IO.Path]::GetFullPath($FilePath)
    [System.IO.File]::WriteAllText($AbsolutePath, $Content, $Utf8NoBom)
}

# Update src-tauri/Cargo.toml
$cargoPath = "src-tauri/Cargo.toml"
if (Test-Path $cargoPath) {
    $content = (Get-Content $cargoPath -Raw -Encoding utf8) -replace '(?m)^version = ".*"', "version = `"$Version`""
    Write-Utf8NoBom $cargoPath $content
    Write-Host "[OK] Updated $cargoPath"
}

# Run cargo check to update Cargo.lock
Write-Host "Running cargo check to update Cargo.lock ..."
cargo check --manifest-path src-tauri/Cargo.toml

Write-Host "[OK] Version bump completed successfully!"
