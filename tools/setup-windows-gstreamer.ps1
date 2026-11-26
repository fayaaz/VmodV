# Shared CI bootstrap for the Windows portable build: installs the pinned
# GStreamer 1.24.13 runtime/devel MSIs and Windows-native pkgconf, exports the
# paths cargo needs, and stages the devel .pc files in a colon-free directory.
# Called by both .github/workflows/vmodv.yml and release.yml; keep the MSI
# versions and checksum in one place. Expects to run from the repo root with
# .gst-cache restored by actions/cache.
$ErrorActionPreference = 'Stop'

$base = 'https://gstreamer.freedesktop.org/data/pkg/windows/1.24.13/msvc'
$runtime = "$base/gstreamer-1.0-msvc-x86_64-1.24.13.msi"
$devel = "$base/gstreamer-1.0-devel-msvc-x86_64-1.24.13.msi"
New-Item -ItemType Directory -Force -Path '.gst-cache' | Out-Null
$runtimeMsi = "$pwd\.gst-cache\gst-runtime.msi"
$develMsi = "$pwd\.gst-cache\gst-devel.msi"
if (-not (Test-Path $runtimeMsi)) {
  Invoke-WebRequest -Uri $runtime -OutFile $runtimeMsi
}
if (-not (Test-Path $develMsi)) {
  Invoke-WebRequest -Uri $devel -OutFile $develMsi
}
$runtimeInstall = Start-Process msiexec.exe -Wait -PassThru -ArgumentList '/i', $runtimeMsi, '/quiet', '/norestart', 'INSTALLDIR=C:\gstreamer\1.0\msvc_x86_64', '/L*V', "$pwd\.gst-cache\gst-runtime-install.log"
if ($runtimeInstall.ExitCode -ne 0) { throw "GStreamer runtime MSI install failed with exit code $($runtimeInstall.ExitCode)" }
$develInstall = Start-Process msiexec.exe -Wait -PassThru -ArgumentList '/i', $develMsi, '/quiet', '/norestart', 'INSTALLDIR=C:\gstreamer\1.0\msvc_x86_64', '/L*V', "$pwd\.gst-cache\gst-devel-install.log"
if ($develInstall.ExitCode -ne 0) { throw "GStreamer devel MSI install failed with exit code $($develInstall.ExitCode)" }

# Chocolatey's feed is flaky; install pinned pkgconf MSI directly and cache it.
$pkgconfMsi = Join-Path $pwd '.gst-cache\pkgconf-x64-3.0.11.msi'
if (-not (Test-Path $pkgconfMsi)) {
  Invoke-WebRequest -Uri 'https://github.com/pkgconf/pkgconf/releases/download/pkgconf-3.0.11/pkgconf-x64-3.0.11.msi' -OutFile $pkgconfMsi
}
$digest = (Get-FileHash -Path $pkgconfMsi -Algorithm SHA256).Hash.ToLowerInvariant()
if ($digest -ne 'ea99c87ab4a3dd8799d47ef196f66ca57d274ec4f49b25d2f48b7c9d9df0eb96') { throw "pkgconf MSI checksum mismatch: $digest" }
$pkgconfInstall = Start-Process msiexec.exe -Wait -PassThru -ArgumentList '/i', $pkgconfMsi, '/quiet', '/norestart'
if ($pkgconfInstall.ExitCode -ne 0) { throw "pkgconf MSI install failed with exit code $($pkgconfInstall.ExitCode)" }

# The MSI updates PATH only for future processes, so the runner's captured
# PATH still lacks pkgconf; refresh it from the registry and export the
# resolved absolute path (a bare name fails in glib-sys).
$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')
$pkgconf = (Get-Command pkgconf.exe -ErrorAction Stop).Source
"PKG_CONFIG=$pkgconf" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
"PKG_CONFIG_PATH=/gst-pkgconfig" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
$libFile = Get-ChildItem -Path 'C:\gstreamer', "$env:SystemDrive\gstreamer" -Filter 'gstvideo-1.0.lib' -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $libFile) { throw 'gstvideo-1.0.lib not found under the GStreamer install' }
$libDir = $libFile.DirectoryName
"RUSTFLAGS=-L$libDir" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
$gstRoot = (Split-Path -Parent $libDir).Replace('\','/')
"GSTREAMER_ROOT=$gstRoot" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append

# pkgconf 3.0.10 also splits PKG_CONFIG_PATH on ':', so a drive-letter path is
# mangled. Stage the devel .pc files into a colon-free root-level gst-pkgconfig
# directory on both the system drive and the workspace drive.
$pcFile = Get-ChildItem -Path 'C:\gstreamer' -Filter 'glib-2.0.pc' -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $pcFile) { throw 'glib-2.0.pc not found under C:\gstreamer after the GStreamer MSI install' }
$gstPc = $pcFile.DirectoryName
$targets = @("$env:SystemDrive\gst-pkgconfig")
$wsDrive = (Get-Location).Drive.Name
if ($wsDrive -ne $env:SystemDrive.TrimEnd(':')) { $targets += "${wsDrive}:\gst-pkgconfig" }
foreach ($target in $targets) {
  New-Item -ItemType Directory -Force -Path $target | Out-Null
  Copy-Item -Path "$gstPc\*.pc" -Destination $target -Force
}

# Put the GStreamer DLLs on PATH for the later cargo build (was a separate
# Git Bash step; writing GITHUB_PATH from pwsh avoids MSYS path rewriting).
"C:/gstreamer/1.0/msvc_x86_64/bin" | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
