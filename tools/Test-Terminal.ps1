# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
Types text with a typo into a terminal prompt and checks that Typomend
corrected it.

.DESCRIPTION
Opens a new terminal window, types the text at its prompt, reads the line back
through UI Automation and compares its end with the expected text. Run
Typomend, or the console example, first:

  cargo run -p typomend-observer --example console

WARNING: this script takes over the foreground window and the keyboard while
it runs.

The script exits with 0 when the line ends with -Expect and with 1 otherwise.

.PARAMETER Terminal
wt opens Windows Terminal. conhost opens the classic console window.

.PARAMETER App
pwsh types `echo 這個因該可以` at a PowerShell prompt. claude types 這個因該可以
into the prompt of Claude Code without sending it. Claude Code starts in plan
mode in the current directory, which it must already trust.

.PARAMETER Mode
unicode sends the text as VK_PACKET, bypassing the IME. bopomofo types it
through Microsoft Bopomofo and presses Enter to commit.

.PARAMETER Keys
Bopomofo key sequence for -Mode bopomofo. Defaults to the keys of
這個因該可以.

.PARAMETER Text
Text for -Mode unicode. Defaults to 這個因該可以.

.PARAMETER Expect
Text the line must end with afterwards. Defaults to the corrected text.

.PARAMETER Keep
Leaves the window open.

.EXAMPLE
.\Test-Terminal.ps1

Types the default sentence through Microsoft Bopomofo into PowerShell in
Windows Terminal.

.EXAMPLE
.\Test-Terminal.ps1 -Terminal conhost -App claude -Mode unicode -Keep

Types the default sentence into Claude Code in a classic console window and
leaves the window open.
#>
param(
  [ValidateSet('wt', 'conhost')] [string]$Terminal = 'wt',
  [ValidateSet('pwsh', 'claude')] [string]$App = 'pwsh',
  [ValidateSet('unicode', 'bopomofo')] [string]$Mode = 'bopomofo',
  # Bopomofo standard layout: 這 5k4, 個 ek4, 因 up<space>, 該 e9<space>, 可 dk3, 以 u3.
  [string]$Keys = '5k4ek4up e9 dk3u3', [string]$Text = '這個因該可以',
  [string]$Expect, [switch]$Keep
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
# Win32 calls for SendInput, window lookup, the foreground window and the IME.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class K {
  [StructLayout(LayoutKind.Sequential)] public struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct IN { public uint type; public KI ki; long pad; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
  [StructLayout(LayoutKind.Sequential)] public struct GTI { public int cb; public uint flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT rc; }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint tid, ref GTI info);
  [DllImport("imm32.dll")] public static extern IntPtr ImmGetDefaultIMEWnd(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr x);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, IN[] i, int s);
  public static IntPtr FocusHwnd() { var g = new GTI(); g.cb = Marshal.SizeOf(g); GetGUIThreadInfo(0, ref g); return g.focus; }
  public static void Key(ushort vk, bool unicode, ushort scan) {
    uint f = unicode ? 4u : 0u;
    var a = new IN[2];
    a[0].type = 1; a[0].ki.vk = vk; a[0].ki.scan = scan; a[0].ki.flags = f;
    a[1].type = 1; a[1].ki.vk = vk; a[1].ki.scan = scan; a[1].ki.flags = f | 2;
    SendInput(2, a, Marshal.SizeOf(typeof(IN)));
  }
}
"@
$A = [System.Windows.Automation.AutomationElement]
$Unit = [System.Windows.Automation.Text.TextUnit]
$End = [System.Windows.Automation.Text.TextPatternRangeEndpoint]

function Send-Unicode([string]$s) {
  foreach ($c in $s.ToCharArray()) { [K]::Key(0, $true, [uint16]$c); Start-Sleep -Milliseconds 60 }
}

# The text of the cursor's row up to the cursor.
function Get-Line($pattern) {
  $caret = $pattern.GetSelection()[0]
  $line = $caret.Clone()
  $line.ExpandToEnclosingUnit($Unit::Line)
  $line.MoveEndpointByRange($End::End, $caret, $End::Start)
  $line.GetText(-1)
}

# A unique title finds the new window without touching any other.
$title = "TypomendTest-$PID"
$pwsh = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
if (-not $pwsh) { $pwsh = (Get-Command powershell).Source }
if ($App -eq 'claude') {
  $prefix = ''
  # Plan mode keeps Claude Code from changing anything, should the prompt
  # ever be sent.
  $command = @((Get-Command claude).Source, '--permission-mode', 'plan')
  $startup = 8
} else {
  $prefix = 'echo '
  $command = @($pwsh, '-NoProfile', '-NoLogo')
  $startup = 2
}
if (-not $Expect) { $Expect = $prefix + '這個應該可以' }
if ($Terminal -eq 'wt') {
  $wt = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\wt.exe'
  Start-Process $wt -ArgumentList (@('-w', 'new', '--title', $title, '--suppressApplicationTitle', '-d', (Get-Location).Path) + $command)
} else {
  # The console takes its title from a shell that sets it, then runs the app.
  $run = if ($App -eq 'claude') { "; & '$($command[0])' $($command[1..9] -join ' '); exit" } else { '' }
  Start-Process conhost.exe -ArgumentList $pwsh, '-NoProfile', '-NoLogo', '-NoExit', '-Command', "`$Host.UI.RawUI.WindowTitle='$title'$run"
}
$window = [IntPtr]::Zero
for ($i = 0; $i -lt 60 -and $window -eq [IntPtr]::Zero; $i++) {
  Start-Sleep -Milliseconds 250
  $window = [K]::FindWindowW([NullString]::Value, $title)
}
if ($window -eq [IntPtr]::Zero) { throw "The $Terminal window did not open." }
# Give the shell or app time to show its prompt.
Start-Sleep -Seconds $startup

# Tapping Alt first lets this process take the foreground, which Windows
# otherwise refuses while someone else has it.
[K]::keybd_event(0x12, 0, 0, 0); [K]::keybd_event(0x12, 0, 2, 0)
[K]::SetForegroundWindow($window) | Out-Null
Start-Sleep -Milliseconds 800
if ([K]::GetForegroundWindow() -ne $window) { throw 'The terminal is not in the foreground; no keys were sent.' }
$pattern = $A::FocusedElement.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)

# WM_IME_CONTROL with IMC_GETCONVERSIONMODE / IMC_SETCONVERSIONMODE. The
# conversion mode is switched only for the test and restored afterwards.
$ime = [K]::ImmGetDefaultIMEWnd([K]::FocusHwnd())
$oldConv = [K]::SendMessageW($ime, 0x283, 1, 0)
try {
  [K]::SendMessageW($ime, 0x283, 2, 0) | Out-Null; Start-Sleep -Milliseconds 100
  Send-Unicode $prefix
  if ($Mode -eq 'unicode') {
    Send-Unicode $Text
  } else {
    [K]::SendMessageW($ime, 0x283, 2, 1) | Out-Null; Start-Sleep -Milliseconds 300
    foreach ($c in $Keys.ToCharArray()) {
      $vk = if ($c -eq ' ') { 0x20 } else { [uint16][char]::ToUpper($c) }
      [K]::Key($vk, $false, 0); Start-Sleep -Milliseconds 120
    }
    Start-Sleep -Milliseconds 1500
    [K]::Key(0x0D, $false, 0)
  }
  Start-Sleep -Seconds 3
  $line = Get-Line $pattern
} finally {
  [K]::SendMessageW($ime, 0x283, 2, $oldConv) | Out-Null
}
"Line: [$line]"
$ok = $line.TrimEnd().EndsWith($Expect)

if (-not $Keep -and $App -eq 'claude') {
  # Closing the window ends Claude Code without sending the prompt.
  [K]::PostMessageW($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
} elseif (-not $Keep -and [K]::GetForegroundWindow() -eq $window) {
  # Escape clears the line in PSReadLine and cmd alike.
  [K]::Key(0x1B, $false, 0); Start-Sleep -Milliseconds 200
  Send-Unicode 'exit'; [K]::Key(0x0D, $false, 0)
}
if ($ok) { 'PASS' } else { "FAIL: expected the line to end with [$Expect]" }
exit [int](-not $ok)
