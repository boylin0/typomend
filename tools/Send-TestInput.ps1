# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
Types the test sentence into a text control to check Typomend end to end.

.DESCRIPTION
Focuses the target control, clears it, types the sentence and prints the text
the control holds afterwards. The sentence has a homophone typo, so Typomend
should have corrected it by then.

WARNING: the target control is cleared first, and this script takes over the
foreground window and the keyboard while it runs.

No key is sent unless the top-level window of the target is in the
foreground, so a failed focus change can never type into an unrelated window.

.PARAMETER Mode
unicode sends the text as VK_PACKET, bypassing the IME. bopomofo types the key
sequence through Microsoft Bopomofo and presses Enter to commit.

.PARAMETER Window
Top-level window handle, as a decimal number.

.PARAMETER AutomationId
The element under -Window to type into. Without it the window itself is
focused, which suits Notepad. The value "focused" types into the element that
already has focus.

.PARAMETER NameLike
Picks the element by a wildcard on its name, for controls that share an
AutomationId such as the File Explorer search box and address bar.

.PARAMETER Keys
Bopomofo key sequence for -Mode bopomofo. Defaults to the keys of
這個因該可以.

.PARAMETER Text
Text for -Mode unicode. Defaults to 這個因該可以.

.EXAMPLE
.\Send-TestInput.ps1 -Window 659722 -Mode unicode

Types the default sentence into the window, which suits Notepad.

.EXAMPLE
.\Send-TestInput.ps1 -Window 659722 -AutomationId focused -Mode bopomofo

Types the default sentence through Microsoft Bopomofo into the element that
has focus, such as the VS Code editor.
#>
param(
  [string]$Mode = 'unicode', [long]$Window, [string]$AutomationId, [string]$NameLike,
  # Bopomofo standard layout: 這 5k4, 個 ek4, 因 up<space>, 該 e9<space>, 可 dk3, 以 u3.
  [string]$Keys = '5k4ek4up e9 dk3u3', [string]$Text = '這個因該可以'
)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
# Win32 calls for SendInput, the foreground window and the IME.
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
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr x);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, IN[] i, int s);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  public static IntPtr Root(IntPtr h) { return GetAncestor(h, 2); }
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
$root = [K]::Root([IntPtr]$Window)
$e = $A::FromHandle([IntPtr]$Window)
if ($AutomationId -eq 'focused') {
  # Type into whatever already has focus, for controls without an
  # AutomationId such as the VS Code editor.
  $e = $A::FocusedElement
} elseif ($AutomationId -or $NameLike) {
  $all = $e.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $e = $all | Where-Object {
    (-not $AutomationId -or $_.Current.AutomationId -eq $AutomationId) -and (-not $NameLike -or $_.Current.Name -like $NameLike)
  } | Select-Object -First 1
  if (-not $e) { throw "No element matches AutomationId '$AutomationId' and name '$NameLike'" }
}

# Bring the window forward. Tapping Alt first lets this process take the
# foreground, which Windows otherwise refuses while someone else has it.
if ([K]::GetForegroundWindow() -ne $root) {
  [K]::keybd_event(0x12,0,0,0); [K]::keybd_event(0x12,0,2,0)
  [K]::SetForegroundWindow($root) | Out-Null
  Start-Sleep -Milliseconds 300
}
$e.SetFocus(); Start-Sleep -Milliseconds 500
if ([K]::Root([K]::GetForegroundWindow()) -ne $root) {
  throw "Target window is not in the foreground; no keys were sent."
}

# Clear the target: Ctrl+A, Delete.
[K]::keybd_event(0x11,0,0,0); [K]::keybd_event(0x41,0,0,0); [K]::keybd_event(0x41,0,2,0); [K]::keybd_event(0x11,0,2,0); [K]::Key(0x2E,$false,0)
Start-Sleep -Milliseconds 300

# WM_IME_CONTROL with IMC_GETCONVERSIONMODE / IMC_SETCONVERSIONMODE. The
# conversion mode is switched only for the test and restored afterwards.
$imeWnd = [K]::ImmGetDefaultIMEWnd([K]::FocusHwnd())
$oldConv = [K]::SendMessageW($imeWnd, 0x283, 1, 0)
if ($Mode -eq 'unicode') {
  # Alphanumeric mode, so the IME holds no composition, as when typing
  # without an input method.
  [K]::SendMessageW($imeWnd, 0x283, 2, 0) | Out-Null; Start-Sleep -Milliseconds 100
  foreach ($c in $Text.ToCharArray()) { [K]::Key(0, $true, [uint16]$c); Start-Sleep -Milliseconds 80 }
} else {
  [K]::SendMessageW($imeWnd, 0x283, 2, 1) | Out-Null; Start-Sleep -Milliseconds 300
  foreach ($c in $Keys.ToCharArray()) {
    $vk = if ($c -eq ' ') { 0x20 } else { [uint16][char]::ToUpper($c) }
    [K]::Key($vk, $false, 0); Start-Sleep -Milliseconds 120
  }
  Start-Sleep -Milliseconds 1500
  [K]::Key(0x0D, $false, 0)
}
Start-Sleep -Seconds 2
[K]::SendMessageW($imeWnd, 0x283, 2, $oldConv) | Out-Null
$tp = $e.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
"Document: [" + $tp.DocumentRange.GetText(-1) + "]"
