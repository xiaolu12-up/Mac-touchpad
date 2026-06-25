[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
    $client = New-Object System.Net.WebClient
    $client.Headers.Add("User-Agent", "Mac-touchpad")
    # Set Security Protocol to TLS 1.2
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12
    $bytes = $client.DownloadData("https://gitee.com/api/v5/repos/lu52/Mac-touchpad/releases/latest")
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    $json = $text | ConvertFrom-Json
    
    $asset = $json.assets | Where-Object { $_.name -like '*.msi' } | Select-Object -First 1
    if (-not $asset) {
        $asset = $json.assets | Where-Object { $_.name -like '*.exe' } | Select-Object -First 1
    }
    $asset_url = if ($asset) { $asset.browser_download_url } else { '' }
    $html_url = 'https://gitee.com/lu52/Mac-touchpad/releases/tag/' + $json.tag_name

    $out = [PSCustomObject]@{
        tag_name = $json.tag_name
        html_url = $html_url
        body = $json.body
        asset_url = $asset_url
    } | ConvertTo-Json
    Write-Host "Success:"
    Write-Host $out
} catch {
    Write-Host "Error: $_"
}
