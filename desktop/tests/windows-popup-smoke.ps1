param(
    [string]$BinaryPath = (Join-Path $PSScriptRoot '../src-tauri/target/release/clashbar-desktop.exe')
)

# Run only on a disposable Windows runner. This checks the built application,
# without selecting a core, importing a profile, or changing system proxy settings.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $IsWindows) { throw 'The native popup smoke requires Windows.' }
$binary = (Resolve-Path -LiteralPath $BinaryPath).Path

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class ClashBarPopupSmoke
{
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MonitorInfo
    {
        public uint Size;
        public Rect Monitor;
        public Rect Work;
        public uint Flags;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr window, int index);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongW")]
    private static extern int GetWindowLong(IntPtr window, int index);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr window, out Rect bounds);
    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool GetMonitorInfoW(IntPtr monitor, ref MonitorInfo info);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static IntPtr[] FindWindows(int processId, string title)
    {
        var result = new List<IntPtr>();
        EnumWindowsProc callback = (window, parameter) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == processId) {
                var text = new StringBuilder(512);
                GetWindowTextW(window, text, text.Capacity);
                if (String.Equals(text.ToString(), title, StringComparison.Ordinal))
                    result.Add(window);
            }
            return true;
        };
        if (!EnumWindows(callback, IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return result.ToArray();
    }

    public static long WindowStyle(IntPtr window, int index)
    {
        return IntPtr.Size == 8 ? GetWindowLongPtr(window, index).ToInt64()
                               : GetWindowLong(window, index);
    }
}
'@

$app = $null
$secondaryProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$previousDpiContext = [IntPtr]::Zero

function Assert-Condition([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-AppRunning {
    $app.Refresh()
    Assert-Condition (-not $app.HasExited) "ClashBar exited unexpectedly (PID $($app.Id))."
}

function Wait-Condition([scriptblock]$Condition, [string]$Description) {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt 15) {
        Assert-AppRunning
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "Timed out after 15 seconds: $Description"
}

function Open-ExistingInstance {
    $secondary = Start-Process -FilePath $binary -WorkingDirectory (Split-Path $binary) -PassThru
    $secondaryProcesses.Add($secondary)
    Assert-Condition ($secondary.WaitForExit(15000)) 'The second launch did not hand off to the existing instance.'
    Assert-Condition ($secondary.ExitCode -eq 0) "The second launch failed with exit code $($secondary.ExitCode)."
    Wait-Condition { [ClashBarPopupSmoke]::IsWindowVisible($script:mainWindow) } 'the existing popup to appear after a second launch'
    $geometryState = [pscustomobject]@{ Signature = ''; Samples = 0 }
    Wait-Condition {
        $rect = [ClashBarPopupSmoke+Rect]::new()
        if (-not [ClashBarPopupSmoke]::IsWindowVisible($script:mainWindow)) {
            $geometryState.Samples = 0
            return $false
        }
        Assert-Condition ([ClashBarPopupSmoke]::GetWindowRect($script:mainWindow, [ref]$rect)) 'Cannot read popup bounds while waiting for layout.'
        $signature = "$($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)"
        if ($signature -eq $geometryState.Signature) { $geometryState.Samples++ }
        else {
            $geometryState.Signature = $signature
            $geometryState.Samples = 1
        }
        return $geometryState.Samples -ge 5
    } 'the shown popup geometry to settle'
}

function Assert-PopupGeometry {
    $bounds = [ClashBarPopupSmoke+Rect]::new()
    Assert-Condition ([ClashBarPopupSmoke]::GetWindowRect($script:mainWindow, [ref]$bounds)) 'GetWindowRect failed.'
    $dpi = [ClashBarPopupSmoke]::GetDpiForWindow($script:mainWindow)
    Assert-Condition ($dpi -gt 0) 'GetDpiForWindow returned zero.'
    $logicalWidth = ($bounds.Right - $bounds.Left) * 96.0 / $dpi
    Assert-Condition ([Math]::Abs($logicalWidth - 360) -le 1) "Expected a 360-point popup, got $logicalWidth at $dpi DPI."

    $monitor = [ClashBarPopupSmoke]::MonitorFromWindow($script:mainWindow, 2)
    Assert-Condition ($monitor -ne [IntPtr]::Zero) 'No monitor was associated with the popup.'
    $info = [ClashBarPopupSmoke+MonitorInfo]::new()
    $info.Size = [System.Runtime.InteropServices.Marshal]::SizeOf($info)
    Assert-Condition ([ClashBarPopupSmoke]::GetMonitorInfoW($monitor, [ref]$info)) 'GetMonitorInfoW failed.'
    Assert-Condition (($bounds.Bottom - $bounds.Top) -gt 100) 'The popup has no usable height.'
    Assert-Condition ($bounds.Left -ge $info.Work.Left -and $bounds.Top -ge $info.Work.Top -and
        $bounds.Right -le $info.Work.Right -and $bounds.Bottom -le $info.Work.Bottom) 'The popup extends outside the monitor work area.'

    Write-Host "Popup bounds: [$($bounds.Left), $($bounds.Top), $($bounds.Right), $($bounds.Bottom)], DPI=$dpi, logical width=$logicalWidth."
}

function Save-PopupScreenshot {
    $bitmap = $null
    $graphics = $null
    try {
        Add-Type -AssemblyName System.Drawing
        $bounds = [ClashBarPopupSmoke+Rect]::new()
        Assert-Condition ([ClashBarPopupSmoke]::GetWindowRect($script:mainWindow, [ref]$bounds)) 'Cannot read screenshot bounds.'
        $bitmap = [System.Drawing.Bitmap]::new($bounds.Right - $bounds.Left, $bounds.Bottom - $bounds.Top)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size, [System.Drawing.CopyPixelOperation]::SourceCopy)
        $outputDirectory = Join-Path $PSScriptRoot '../test-results'
        [void](New-Item -ItemType Directory -Path $outputDirectory -Force)
        $imagePath = Join-Path $outputDirectory 'native-popup.png'
        $bitmap.Save($imagePath, [System.Drawing.Imaging.ImageFormat]::Png)
        Write-Host "Native WebView2 screenshot: $imagePath"
    }
    catch {
        # Some runners expose windows without a capturable screen. Native assertions
        # remain mandatory; this additional rendering diagnostic is best effort.
        Write-Warning "Native popup screenshot unavailable: $_"
    }
    finally {
        if ($null -ne $graphics) { $graphics.Dispose() }
        if ($null -ne $bitmap) { $bitmap.Dispose() }
    }
}

try {
    # GetWindowRect is DPI-virtualized; measure in physical pixels before applying
    # the window's own DPI. Restore the runner thread's context in finally.
    # https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getwindowrect
    $previousDpiContext = [ClashBarPopupSmoke]::SetThreadDpiAwarenessContext([IntPtr]::new(-4))
    Assert-Condition ($previousDpiContext -ne [IntPtr]::Zero) 'Cannot enable per-monitor DPI awareness for the smoke test.'

    $app = Start-Process -FilePath $binary -WorkingDirectory (Split-Path $binary) -PassThru
    Wait-Condition { @([ClashBarPopupSmoke]::FindWindows($app.Id, 'ClashBar')).Count -eq 1 } 'the main native window to be created'
    $script:mainWindow = [ClashBarPopupSmoke]::FindWindows($app.Id, 'ClashBar')[0]

    # Check a stable hidden startup, not merely a hidden window during creation.
    for ($sample = 0; $sample -lt 10; $sample++) {
        Assert-AppRunning
        Assert-Condition (-not [ClashBarPopupSmoke]::IsWindowVisible($script:mainWindow)) 'The app showed a window at startup instead of staying in the tray.'
        foreach ($menu in [ClashBarPopupSmoke]::FindWindows($app.Id, 'ClashBar Menu')) {
            Assert-Condition (-not [ClashBarPopupSmoke]::IsWindowVisible($menu)) 'The attached menu showed at startup.'
        }
        Start-Sleep -Milliseconds 100
    }

    Open-ExistingInstance
    $style = [ClashBarPopupSmoke]::WindowStyle($script:mainWindow, -16)
    $extendedStyle = [ClashBarPopupSmoke]::WindowStyle($script:mainWindow, -20)
    Assert-Condition (($style -band 0x00C00000) -eq 0) 'The popup has a native title bar (WS_CAPTION).'
    Assert-Condition (($style -band 0x00040000) -eq 0) 'The popup has a resize frame (WS_THICKFRAME).'
    Assert-Condition (($extendedStyle -band 0x00000080) -ne 0) 'The popup is not a tool window (WS_EX_TOOLWINDOW).'
    Assert-Condition (($extendedStyle -band 0x00040000) -eq 0) 'The popup forces a taskbar button (WS_EX_APPWINDOW).'
    Assert-PopupGeometry

    Assert-Condition ([ClashBarPopupSmoke]::PostMessageW($script:mainWindow, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) 'Posting WM_CLOSE failed.'
    Wait-Condition { -not [ClashBarPopupSmoke]::IsWindowVisible($script:mainWindow) } 'WM_CLOSE to hide the popup'
    Start-Sleep -Milliseconds 300
    Assert-AppRunning

    Open-ExistingInstance
    Assert-PopupGeometry
    Assert-AppRunning
    Save-PopupScreenshot
    Write-Host 'PASS: native hidden startup, frameless tray window, 360-point sizing, work-area placement, close-to-hide, and single-instance reopen.'
}
finally {
    # Only terminate processes started by this smoke, including their WebView2 children.
    foreach ($ownedProcess in $secondaryProcesses.ToArray() + @($app)) {
        if ($null -ne $ownedProcess) {
            try {
                if (-not $ownedProcess.HasExited) {
                    $ownedProcess.Kill($true)
                    [void]$ownedProcess.WaitForExit(5000)
                }
            }
            catch { Write-Warning "Could not terminate owned smoke process $($ownedProcess.Id): $_" }
            finally { $ownedProcess.Dispose() }
        }
    }
    if ($previousDpiContext -ne [IntPtr]::Zero) {
        [void][ClashBarPopupSmoke]::SetThreadDpiAwarenessContext($previousDpiContext)
    }
}
