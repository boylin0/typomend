// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// UiaProbe reports which UI Automation capabilities a text control exposes,
// which decides whether Typomend can read, track IME composition in, and
// write to that control.
//
// Usage: UiaProbe <hwnd> <AutomationId | - | focused | class:Name> [--events] [--nofocus]
//
// The element is located under the window, focused, and then re-read through
// GetFocusedElement so the report matches what Typomend sees.
//
//   <hwnd>      top-level window handle, as a decimal number
//   --events    keep running and log text-changed events until killed
//   --nofocus   leave focus alone so the probe can listen while the user types

using System.Text;
using Interop.UIAutomationClient;

Console.OutputEncoding = Encoding.UTF8;

var uia = new CUIAutomation8();
var window = uia.ElementFromHandle((IntPtr)long.Parse(args[0]));
var cond = uia.CreatePropertyCondition(UIA_PropertyIds.UIA_AutomationIdPropertyId, args[1]);

// An AutomationId of "-" targets the window itself, which suits Notepad whose
// editor is its own HWND.
// "focused" probes whatever already has focus, for controls without an
// AutomationId such as the VS Code editor.
IUIAutomationElement? target = args[1] switch
{
    "-" => window,
    "focused" => uia.GetFocusedElement(),
    // "class:<name>" matches by ClassName, e.g. class:native-edit-context.
    var s when s.StartsWith("class:") => window.FindFirst(TreeScope.TreeScope_Descendants,
        uia.CreatePropertyCondition(UIA_PropertyIds.UIA_ClassNamePropertyId, s["class:".Length..])),
    _ => null,
};
// Chromium builds its accessibility tree lazily after the first UIA client
// connects, so the first lookups may come back empty. Retry for five seconds.
for (int i = 0; i < 20 && target == null; i++)
{
    target = window.FindFirst(TreeScope.TreeScope_Descendants, cond);
    if (target == null)
        Thread.Sleep(250);
}
if (target == null)
{
    Console.WriteLine($"AutomationId '{args[1]}' not found");
    return 1;
}

// Focus the target and re-read it, unless --nofocus keeps focus where it is.
IUIAutomationElement el;
if (args.Contains("--nofocus"))
{
    el = target;
}
else
{
    target.SetFocus();
    Thread.Sleep(300);
    el = uia.GetFocusedElement();
}
Console.WriteLine($"focused: name={el.CurrentName} id={el.CurrentAutomationId} class={el.CurrentClassName} " +
    $"ctl={el.CurrentControlType} fw={el.CurrentFrameworkId} hwnd=0x{el.CurrentNativeWindowHandle:X} pid={el.CurrentProcessId}");

foreach (var (name, id) in new[]
{
    ("TextPattern", UIA_PatternIds.UIA_TextPatternId),
    ("TextPattern2", UIA_PatternIds.UIA_TextPattern2Id),
    ("TextEditPattern", UIA_PatternIds.UIA_TextEditPatternId),
    ("ValuePattern", UIA_PatternIds.UIA_ValuePatternId),
})
{
    Console.WriteLine($"  {name,-16} {el.GetCurrentPattern(id) != null}");
}

if (el.GetCurrentPattern(UIA_PatternIds.UIA_TextPatternId) is IUIAutomationTextPattern tp)
{
    Console.WriteLine($"  document=[{tp.DocumentRange.GetText(-1)}] selections={tp.GetSelection().Length}");
}
if (el.GetCurrentPattern(UIA_PatternIds.UIA_TextEditPatternId) is IUIAutomationTextEditPattern edit)
{
    Console.WriteLine($"  activeComposition=[{edit.GetActiveComposition()?.GetText(-1)}] " +
        $"conversionTarget=[{edit.GetConversionTarget()?.GetText(-1)}]");
}

if (args.Contains("--events"))
{
    // Logs TextEdit text-changed events (composition updates and finalization)
    // and plain text-changed events until the process is killed.
    var handler = new TextEditHandler();
    ((IUIAutomation3)uia).AddTextEditTextChangedEventHandler(el, TreeScope.TreeScope_Subtree, TextEditChangeType.TextEditChangeType_None, null, handler);
    ((IUIAutomation3)uia).AddTextEditTextChangedEventHandler(el, TreeScope.TreeScope_Subtree, TextEditChangeType.TextEditChangeType_Composition, null, handler);
    ((IUIAutomation3)uia).AddTextEditTextChangedEventHandler(el, TreeScope.TreeScope_Subtree, TextEditChangeType.TextEditChangeType_CompositionFinalized, null, handler);
    ((IUIAutomation3)uia).AddTextEditTextChangedEventHandler(el, TreeScope.TreeScope_Subtree, TextEditChangeType.TextEditChangeType_AutoCorrect, null, handler);
    uia.AddAutomationEventHandler(UIA_EventIds.UIA_Text_TextChangedEventId, el, TreeScope.TreeScope_Subtree, null, handler);
    Console.WriteLine("listening");
    Thread.Sleep(Timeout.Infinite);
}
return 0;

/// <summary>
/// Prints every text-changed event it receives, with the element that sent it.
/// </summary>
class TextEditHandler : IUIAutomationTextEditTextChangedEventHandler, IUIAutomationEventHandler
{
    public void HandleTextEditTextChangedEvent(IUIAutomationElement sender, TextEditChangeType type, string[] changes) =>
        Console.WriteLine($"{DateTime.Now:HH:mm:ss.fff} TextEdit {type}: [{string.Join("|", changes ?? [])}] from {Describe(sender)}");

    public void HandleAutomationEvent(IUIAutomationElement sender, int eventId) =>
        Console.WriteLine($"{DateTime.Now:HH:mm:ss.fff} TextChanged from {Describe(sender)}");

    private static readonly CUIAutomation8 Uia = new();

    /// <summary>
    /// Describes the sender by class and runtime id, and says whether it is the
    /// element that currently has keyboard focus.
    /// </summary>
    private static string Describe(IUIAutomationElement e)
    {
        try
        {
            var focused = Uia.GetFocusedElement();
            bool same = Uia.CompareElements(e, focused) != 0;
            string id = string.Join(".", (int[])e.GetRuntimeId());
            string focusId = string.Join(".", (int[])focused.GetRuntimeId());
            return $"{e.CurrentClassName} [{id}] focused={same} (focus is {focused.CurrentClassName} [{focusId}])";
        }
        catch (System.Runtime.InteropServices.COMException) { return "?"; }
    }
}
