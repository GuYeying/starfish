# PrintWindow 窗口内容捕获（无视遮挡/位置；参数：进程名, 输出路径, 等待秒）
param([string]$ProcName = "pygame_test_rect", [string]$OutPath = "D:\Projects\Rust\starfish\win_capture.png", [int]$WaitSec = 3)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class PW {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
}
'@
$p = Get-Process $ProcName -ErrorAction Stop
$h = $p.MainWindowHandle
$r = New-Object PW+RECT
[PW]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $hh = $r.B - $r.T
if ($w -le 0 -or $hh -le 0) { Write-Output "ERR: 窗口尺寸异常 ${w}x${hh}"; exit 1 }
$b = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($b)
$dc = $g.GetHdc()
[PW]::PrintWindow($h, $dc, 2) | Out-Null   # 2 = PW_RENDERFULLCONTENT（含 GPU 合成内容）
$g.ReleaseHdc($dc)
$g.Dispose()
$b.Save($OutPath)
$g2 = $null
Write-Output "captured ${w}x${hh} -> $OutPath"
