# 桌面 resize 自愈验证 v2：Get-Process 拿主窗口句柄 → 强改 500×900 → 截窗口区域
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
}
"@
$p = Get-Process binding_probe -ErrorAction Stop
$h = $p.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Output 'ERR: no main window handle'; exit 1 }
[Win]::MoveWindow($h, 100, 100, 500, 900, $true) | Out-Null
Write-Output "resized HWND $h to 500x900, waiting 2s..."
Start-Sleep -Seconds 2
$r = New-Object Win+RECT
[Win]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $hh = $r.B - $r.T
Write-Output "window rect: ${w}x${hh} @($($r.L),$($r.T))"
$b = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size)
$b.Save('D:\Projects\Rust\starfish\resize_test.png')
$g.Dispose(); $b.Dispose()
Write-Output 'captured resize_test.png'
