#!/usr/bin/env python3
"""Bounded native Desktop acceptance in an existing validation session.

Uses the installed NSIS product, Windows UI Automation and window screenshots.
It never injects browser scripts, creates a login or changes native data roots.
"""
import json
import base64
import hashlib
import ctypes
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.parse import urlsplit


def desktop_state():
    """Read the input desktop in the selected user's actual session; never unlock it."""
    user = ctypes.WinDLL('user32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    user.OpenInputDesktop.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
    user.OpenInputDesktop.restype = ctypes.c_void_p
    user.GetThreadDesktop.argtypes = [ctypes.c_ulong]
    user.GetThreadDesktop.restype = ctypes.c_void_p
    user.GetUserObjectInformationW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p,
                                              ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong)]
    user.CloseDesktop.argtypes = [ctypes.c_void_p]
    user.GetForegroundWindow.restype = ctypes.c_void_p
    def name(handle):
        text = ctypes.create_unicode_buffer(256)
        size = ctypes.c_ulong()
        if not user.GetUserObjectInformationW(handle, 2, text, ctypes.sizeof(text), ctypes.byref(size)):
            raise ctypes.WinError(ctypes.get_last_error())
        return text.value
    current = name(user.GetThreadDesktop(kernel.GetCurrentThreadId()))
    input_desktop = user.OpenInputDesktop(0, False, 1)
    if not input_desktop:
        return dict(thread_desktop=current, input_desktop=None, interactive=False,
                    win32_error=ctypes.get_last_error(), foreground=user.GetForegroundWindow())
    try:
        actual = name(input_desktop)
        return dict(thread_desktop=current, input_desktop=actual, interactive=actual == current == 'Default',
                    foreground=user.GetForegroundWindow())
    finally:
        user.CloseDesktop(input_desktop)


UI = r'''
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class DesktopWindow {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct MenuBar {public uint Size;public Rect Bounds;public IntPtr Menu,Window;public uint Flags;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;public Point(int x,int y){X=x;Y=y;}}
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int command);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool GetMenuBarInfo(IntPtr window,int kind,int item,ref MenuBar info);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr menu,uint item,System.Text.StringBuilder text,int capacity,uint flags);
 [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr menu,uint item,uint flags);
 [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr window,IntPtr menu,uint item,out Rect rect);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
}
'@
$q=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ui-request.json') -Raw | ConvertFrom-Json
$p=Get-Process -Id $q.pid -ErrorAction Stop
if($p.Path -ne $q.image){throw 'Desktop image changed'}
$p.Refresh()
if($q.action -eq 'tray-observe') {
 $desktop=[Windows.Automation.AutomationElement]::RootElement
 $tops=@($desktop.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition))
 $windows=@($tops | Where-Object {($_.Current.ClassName -in @('Shell_TrayWnd','NotifyIconOverflowWindow','TopLevelWindowForOverflowXamlIsland') -or ($_.Current.ClassName -eq '#32768' -and $_.Current.ProcessId -eq $q.pid)) -and -not $_.Current.IsOffscreen})
 $nodes=@(foreach($w in $windows){foreach($e in $w.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)){
  @{name=$e.Current.Name;type=$e.Current.ControlType.ProgrammaticName;automation_id=$e.Current.AutomationId;offscreen=$e.Current.IsOffscreen;runtime_id=($e.GetRuntimeId() -join '.');rect=@($e.Current.BoundingRectangle.X,$e.Current.BoundingRectangle.Y,$e.Current.BoundingRectangle.Width,$e.Current.BoundingRectangle.Height)}
 }})
 if($nodes.Count -gt 1600){throw 'Notification observation exceeds its bound'}
 $rect=[Windows.Rect]::Empty
 foreach($w in $windows){$rect.Union($w.Current.BoundingRectangle)}
 if($rect.Width -lt 1 -or $rect.Height -lt 1 -or $rect.Width -gt 4096 -or $rect.Height -gt 4096){throw 'Notification area is not capturable'}
 $bitmap=New-Object Drawing.Bitmap ([int]$rect.Width),([int]$rect.Height)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 try {$graphics.CopyFromScreen([int]$rect.X,[int]$rect.Y,0,0,$bitmap.Size);$bitmap.Save((Join-Path $PSScriptRoot 'tray.png'),[Drawing.Imaging.ImageFormat]::Png)}
 finally {$graphics.Dispose();$bitmap.Dispose()}
 @{pid=$p.Id;nodes=$nodes;top_windows=@(foreach($w in $tops){@{name=$w.Current.Name;class=$w.Current.ClassName;pid=$w.Current.ProcessId}})}|ConvertTo-Json -Depth 6 -Compress
 exit 0
}
if($q.action -eq 'quit') {
 $desktop=[Windows.Automation.AutomationElement]::RootElement
 $class=[Windows.Automation.AutomationElement]::ClassNameProperty
 $name=[Windows.Automation.AutomationElement]::NameProperty
 $children=[Windows.Automation.TreeScope]::Children
 $descendants=[Windows.Automation.TreeScope]::Descendants
 function NativeQuit {
  $menus=@($desktop.FindAll($children,[Windows.Automation.Condition]::TrueCondition) | Where-Object {$_.Current.ProcessId -eq $q.pid -and $_.Current.ClassName -eq '#32768' -and -not $_.Current.IsOffscreen})
  if($menus.Count -eq 0){return $false}
  if($menus.Count -ne 1){throw 'More than one measured-process popup menu is visible'}
  $window=[IntPtr]$menus[0].Current.NativeWindowHandle
  $bar=[DesktopWindow+MenuBar]::new()
  $bar.Size=[Runtime.InteropServices.Marshal]::SizeOf($bar)
  # Documented OBJID_CLIENT reads the real popup menu; no app command is injected.
  if(-not [DesktopWindow]::GetMenuBarInfo($window,-4,0,[ref]$bar)){throw 'Cannot inspect the owned native popup menu'}
  $count=[DesktopWindow]::GetMenuItemCount($bar.Menu)
  if($count -lt 1 -or $count -gt 32){throw 'Native popup menu exceeds its bound'}
  $items=@(for($i=0;$i -lt $count;$i++) {
   $text=[Text.StringBuilder]::new(512)
   [void][DesktopWindow]::GetMenuString($bar.Menu,[uint32]$i,$text,512,0x400)
   $rect=[DesktopWindow+Rect]::new()
   if(-not [DesktopWindow]::GetMenuItemRect([IntPtr]::Zero,$bar.Menu,[uint32]$i,[ref]$rect)){throw 'Native menu geometry is unavailable'}
   @{name=$text.ToString();index=$i;state=[DesktopWindow]::GetMenuState($bar.Menu,[uint32]$i,0x400);left=$rect.Left;top=$rect.Top;right=$rect.Right;bottom=$rect.Bottom}
  })
  $items | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'menu-native-before.json') -Encoding UTF8
  $choice=@($items | Where-Object {$_.name.Replace('&','') -eq 'Quit'})
  if($choice.Count -ne 1 -or ($choice[0].state -band 3) -ne 0){throw 'One enabled native Quit item is required'}
  $item=$choice[0]
  $point=[DesktopWindow+Point]::new([int](($item.left+$item.right)/2),[int](($item.top+$item.bottom)/2))
  if([DesktopWindow]::WindowFromPoint($point) -ne $window){throw 'The observed native menu is obscured'}
  [void][DesktopWindow]::SetCursorPos($point.X,$point.Y)
  [DesktopWindow]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  [DesktopWindow]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  if(-not $p.WaitForExit(10000)){throw 'Native Quit did not exit the measured Desktop'}
  return $true
 }
 if(NativeQuit){@{quit=$true;pid=$p.Id;menu_item='Quit';method='native popup readback and mouse click'}|ConvertTo-Json -Compress;exit 0}
 function QuitItems {
  $windows=$desktop.FindAll($children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,[int]$q.pid))
  $items=@()
  foreach($w in $windows){$items+=@($w.FindAll($descendants,[Windows.Automation.Condition]::TrueCondition))}
  $menuItems=@($items | Where-Object {$_.Current.ControlType -eq [Windows.Automation.ControlType]::MenuItem -and -not $_.Current.IsOffscreen} | Sort-Object {($_.GetRuntimeId() -join '.')} -Unique)
  @($menuItems | ForEach-Object {@{name=$_.Current.Name;runtime_id=($_.GetRuntimeId() -join '.')}}) | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'menu-before.json') -Encoding UTF8
  return @($menuItems | Where-Object {$_.Current.Name -eq 'Quit'})
 }
 $quit=@(QuitItems)
 if($quit.Count -eq 0) {
 $bar=$desktop.FindFirst($children,[Windows.Automation.PropertyCondition]::new($class,'Shell_TrayWnd'))
 if($null -eq $bar){throw 'No native notification area'}
 $find=[Windows.Automation.PropertyCondition]::new($name,'HiRoute')
 $icons=@($bar.FindAll($descendants,$find) | Where-Object {-not $_.Current.IsOffscreen})
 $overflowClasses=@('NotifyIconOverflowWindow','TopLevelWindowForOverflowXamlIsland')
 foreach($className in $overflowClasses) {
  $overflow=$desktop.FindFirst($children,[Windows.Automation.PropertyCondition]::new($class,$className))
  if($null -ne $overflow -and -not $overflow.Current.IsOffscreen){$icons+=@($overflow.FindAll($descendants,$find) | Where-Object {-not $_.Current.IsOffscreen})}
 }
 if($icons.Count -eq 0) {
  $chevrons=@($bar.FindAll($descendants,[Windows.Automation.Condition]::TrueCondition) | Where-Object {
   $_.Current.AutomationId -eq 'SystemTrayIcon' -and $_.Current.Name -in @('Show hidden icons','显示隐藏的图标')
  })
  if($chevrons.Count -ne 1){throw 'Observe the notification area before continuing; icon is unavailable'}
  $chevrons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
  Start-Sleep -Milliseconds 500
  foreach($className in $overflowClasses) {
   $overflow=$desktop.FindFirst($children,[Windows.Automation.PropertyCondition]::new($class,$className))
   if($null -ne $overflow -and -not $overflow.Current.IsOffscreen){$icons+=@($overflow.FindAll($descendants,$find) | Where-Object {-not $_.Current.IsOffscreen})}
  }
 }
 if($icons.Count -ne 1){throw 'Exactly one visible HiRoute notification icon is required'}
 $icons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
 Start-Sleep -Milliseconds 500
 if(NativeQuit){@{quit=$true;pid=$p.Id;menu_item='Quit';method='native popup readback and mouse click'}|ConvertTo-Json -Compress;exit 0}
 $quit=@(QuitItems)
 }
 if($quit.Count -ne 1){throw "The measured Desktop exposed $($quit.Count) native Quit menu items; inspect menu-before.json"}
 $quit[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
 if(-not $p.WaitForExit(10000)){throw 'Native Quit did not exit the measured Desktop'}
 @{quit=$true;pid=$p.Id;menu_item='Quit'}|ConvertTo-Json -Compress
 exit 0
}
if($p.MainWindowHandle -eq 0){throw 'No visible native Desktop window'}
[void][DesktopWindow]::ShowWindow($p.MainWindowHandle,9)
[void][DesktopWindow]::SetForegroundWindow($p.MainWindowHandle)
Start-Sleep -Milliseconds 500
if([DesktopWindow]::GetForegroundWindow() -ne $p.MainWindowHandle) {
 $r=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle).Current.BoundingRectangle
 $point=[DesktopWindow+Point]::new([int]($r.X+100),[int]($r.Y+14))
 $owner=[uint32]0
 [void][DesktopWindow]::GetWindowThreadProcessId([DesktopWindow]::WindowFromPoint($point),[ref]$owner)
 if($owner -eq $q.pid) {
  [void][DesktopWindow]::SetCursorPos($point.X,$point.Y)
  [DesktopWindow]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  [DesktopWindow]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  Start-Sleep -Milliseconds 500
 }
}
if([DesktopWindow]::GetForegroundWindow() -ne $p.MainWindowHandle) {
 # Windows may deny a background observer foreground activation. Use the
 # actual taskbar button for this installed AppUserModelID, without attaching
 # another application's input queue or sending it keyboard events.
 $desktop=[Windows.Automation.AutomationElement]::RootElement
 $bar=$desktop.FindFirst([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ClassNameProperty,'Shell_TrayWnd'))
 if($null -ne $bar) {
  $buttons=@($bar.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty,'Appid: ai.hiroute.desktop')) | Where-Object {-not $_.Current.IsOffscreen -and $_.Current.Name.StartsWith('HiRoute')})
  if($buttons.Count -eq 1) {
   if([DesktopWindow]::GetForegroundWindow() -ne $p.MainWindowHandle) {
    $r=$buttons[0].Current.BoundingRectangle
    if($r.Width -lt 2 -or $r.Height -lt 2){throw 'The HiRoute taskbar button has no visible geometry'}
    $point=[DesktopWindow+Point]::new([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))
    $owner=[uint32]0
    [void][DesktopWindow]::GetWindowThreadProcessId([DesktopWindow]::WindowFromPoint($point),[ref]$owner)
    if($owner -ne $bar.Current.ProcessId){throw 'The observed HiRoute taskbar button is obscured'}
    [void][DesktopWindow]::SetCursorPos($point.X,$point.Y)
    [DesktopWindow]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    [DesktopWindow]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 750
   }
  }
 }
}
if([DesktopWindow]::GetForegroundWindow() -ne $p.MainWindowHandle){
 $foreground=[Windows.Automation.AutomationElement]::FromHandle([DesktopWindow]::GetForegroundWindow())
 @{expected=$p.MainWindowHandle.ToInt64();foreground=@{handle=[DesktopWindow]::GetForegroundWindow().ToInt64();pid=$foreground.Current.ProcessId;name=$foreground.Current.Name;class=$foreground.Current.ClassName}}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $PSScriptRoot 'focus.json') -Encoding UTF8
 throw 'Native Desktop is not foreground; inspect focus.json'
}
$root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
function Observe($root) {
 $all=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 # Some WebView2 fragment roots are reachable by a native accessibility hit
 # test before their host HWND exposes a complete descendant tree.
 $r=$root.Current.BoundingRectangle
 $hit=[Windows.Automation.AutomationElement]::FromPoint([Windows.Point]::new($r.X+80,$r.Y+160))
 if($hit.Current.ProcessId -eq $q.pid -or $hit.Current.FrameworkId -eq 'Chrome') {
  $top=$hit
  for($i=0;$i -lt 40;$i++) {
   if($top.Current.ControlType -eq [Windows.Automation.ControlType]::Document){
    return @($all)+@($top)+@($top.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
   }
   $top=[Windows.Automation.TreeWalker]::RawViewWalker.GetParent($top)
   if($null -eq $top -or $top.Equals($root)){break}
  }
 }
 return @($all)
}
$all=@(Observe $root)
if($all.Count -gt 1600){throw 'UI Automation tree exceeds observation bound'}
if($q.action -in @('invoke','text','close','click','scroll')) {
 if($q.action -eq 'close') {
  $root.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close()
 } elseif($q.action -in @('click','scroll')) {
  $r=$root.Current.BoundingRectangle
  if($q.window -ne $p.MainWindowHandle.ToInt64() -or $q.width -ne $r.Width -or $q.height -ne $r.Height){throw 'Observed window geometry changed'}
  if($q.x -lt 0 -or $q.y -lt 0 -or $q.x -ge $r.Width -or $q.y -ge $r.Height){throw 'Click is outside the observed window'}
  [void][DesktopWindow]::SetCursorPos([int]($r.X+$q.x),[int]($r.Y+$q.y))
  if($q.action -eq 'scroll') {
   if($q.direction -notin @('up','down')){throw 'Invalid scroll direction'}
   $delta=if($q.direction -eq 'down'){-480}else{480}
   $data=[BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$delta),0)
   [DesktopWindow]::mouse_event(0x0800,0,0,$data,[UIntPtr]::Zero)
  } else {
   [DesktopWindow]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
   [DesktopWindow]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  }
 } else {
  $matches=@($all | Where-Object {($_.GetRuntimeId() -join '.') -eq $q.element.runtime_id} | Select-Object -First 1)
  if($matches.Count -ne 1){throw 'Observed element is no longer unique'}
  $e=$matches[0]
  if($e.Current.Name -ne $q.element.name -or $e.Current.ControlType.ProgrammaticName -ne $q.element.type){throw 'Observed element changed'}
  if(-not $e.Current.IsEnabled -or $e.Current.IsOffscreen){throw 'Element is not interactable'}
  if($q.action -eq 'text') {
   $e.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($q.text)
  } else {
   $pattern=$null
   if($e.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
   elseif($e.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)){$pattern.Select()}
   elseif($e.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
   elseif($e.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)){$pattern.Expand()}
   else {throw 'Element has no supported native action'}
  }
 }
 Start-Sleep -Milliseconds 750
 $p.Refresh()
 if($p.MainWindowHandle -eq 0){@{closed=$true;pid=$p.Id}|ConvertTo-Json -Compress;exit 0}
 $root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
 $all=@(Observe $root)
}
$nodes=@(foreach($e in $all){
 $c=$e.Current
 @{runtime_id=($e.GetRuntimeId() -join '.');name=$c.Name;type=$c.ControlType.ProgrammaticName;
   automation_id=$c.AutomationId;enabled=$c.IsEnabled;offscreen=$c.IsOffscreen;
   rect=@($c.BoundingRectangle.X,$c.BoundingRectangle.Y,$c.BoundingRectangle.Width,$c.BoundingRectangle.Height)}
})
$r=$root.Current.BoundingRectangle
if($r.Width -lt 100 -or $r.Height -lt 100 -or $r.Width -gt 4096 -or $r.Height -gt 4096){throw 'Window is not capturable'}
$bitmap=New-Object Drawing.Bitmap ([int]$r.Width),([int]$r.Height)
$graphics=[Drawing.Graphics]::FromImage($bitmap)
try {
 $graphics.CopyFromScreen([int]$r.X,[int]$r.Y,0,0,$bitmap.Size)
 $bitmap.Save((Join-Path $PSScriptRoot 'window.png'),[Drawing.Imaging.ImageFormat]::Png)
} finally {$graphics.Dispose();$bitmap.Dispose()}
@{pid=$p.Id;handle=$p.MainWindowHandle.ToInt64();width=$r.Width;height=$r.Height;window=$root.Current.Name;nodes=$nodes}|ConvertTo-Json -Depth 6 -Compress
'''


def installed(helper, product, pm):
    """Admit actual files, not a process name, window title or a marker alone."""
    roots = helper.locations(Path.home(), Path(os.environ['LOCALAPPDATA']))
    facts = helper.inspect_installation(Path.home(), Path(os.environ['LOCALAPPDATA']))
    record = facts.get('marker', {}).get('record', {})
    install = helper.plain(Path(record.get('install_root', '')))
    if (not install.is_absolute() or install.parent != Path.home() / '.local/lib/hiroute'
            or not install.name.endswith(pm['source_revision'][:12])
            or record.get('target') != pm['target']):
        raise ValueError('Installed Desktop does not match the selected product')
    for name in ('hiroute.exe', 'hirouted.exe'):
        if helper.digest(helper.plain(install / name)) != pm['files'][name]['sha256']:
            raise ValueError('Installed product binary changed')
    shell = helper.plain(roots['programs'] / 'hiroute-shell.exe')
    expected = facts.get('shell-package.json', {}).get('record', {}).get('files', {})
    if (helper.digest(shell) != pm['shell_sha256'] or expected.get('hiroute-shell.exe') != pm['shell_sha256']
            or facts.get('shell_inventory_mismatches')):
        raise ValueError('Installed shell differs from measured NSIS member')
    return roots, facts, shell


def ui_action(run, request, shell, pid, element=None, observed=None):
    state = desktop_state()
    (run / 'desktop-state.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    if not state['interactive']:
        raise RuntimeError('The existing session input desktop is unavailable; unlock it before native UI acceptance')
    query = dict(pid=pid, image=str(shell), action=request['desktop_action'], element=element,
                 text=request.get('text'))
    if request['desktop_action'] in ('click', 'scroll'):
        if request['desktop_action'] == 'scroll' and request.get('scroll_direction') not in ('up', 'down'):
            raise ValueError('Scrolling requires an explicit direction')
        query.update(window=observed['handle'], width=observed['width'], height=observed['height'],
                     x=request['x'], y=request['y'], direction=request.get('scroll_direction'))
    (run / 'ui-request.json').write_text(json.dumps(query), encoding='utf-8')
    script = run / 'ui.ps1'
    script.write_text(UI, encoding='utf-8-sig')
    completed = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-File', str(script)],
                               capture_output=True, timeout=60, creationflags=0x08000000)
    (run / 'ui.stderr').write_bytes(completed.stderr)
    if completed.returncode:
        raise RuntimeError('Native UI Automation failed; inspect ui.stderr before retrying')
    observed = json.loads(completed.stdout.decode('utf-8-sig'))
    (run / 'ui.json').write_text(json.dumps(observed, ensure_ascii=False, indent=2), encoding='utf-8')
    return observed


def codex_config_path(home, profile):
    if profile not in (None, 'hiroute'):
        raise ValueError('Only the product-owned Codex profile is accepted')
    return home / '.codex' / ('hiroute.config.toml' if profile else 'config.toml')


def codex_gateway(document, profile):
    codex_config_path(Path.home(), profile)
    selected = document
    target = document.get('model_providers', {}).get('hiroute', {}).get('base_url', '')
    address = urlsplit(target)
    if (selected.get('model_provider') != 'hiroute' or address.scheme != 'http'
            or address.hostname not in ('127.0.0.1', 'localhost', '::1')
            or not address.port or address.username or address.password):
        raise ValueError('Saved Codex configuration must select the local HiRoute Gateway')
    return target


def codex_call_evidence(stdout, expected, directory_token):
    events = [json.loads(line) for line in stdout.decode('utf-8-sig').splitlines() if line.strip()]
    items = [event.get('item', {}) for event in events if event.get('type') == 'item.completed']
    return dict(turn_completed=any(event.get('type') == 'turn.completed' for event in events),
                answer=any(item.get('type') == 'agent_message' and item.get('text', '').strip() == expected for item in items),
                shell_tool=any(item.get('type') == 'command_execution' and item.get('status') == 'completed'
                               and item.get('exit_code') == 0
                               and re.search(r'\bGet-Location\b', item.get('command', ''), re.I)
                               and directory_token in item.get('aggregated_output', '') for item in items))


def codex_process(run, command, environment):
    try:
        done = subprocess.run(command, cwd=run, stdin=subprocess.DEVNULL, capture_output=True,
                              timeout=150, env=environment, creationflags=0x08000000)
    except subprocess.TimeoutExpired as error:
        (run / 'codex.stdout').write_bytes(error.stdout or b'')
        (run / 'codex.stderr').write_bytes(error.stderr or b'')
        raise
    (run / 'codex.stdout').write_bytes(done.stdout)
    (run / 'codex.stderr').write_bytes(done.stderr)
    return done


def codex_call(run, request, helper):
    environment = os.environ.copy()
    environment.update(request['config']['environment'])
    environment['PATH'] = ';'.join(request['config']['path']) + ';' + environment['PATH']
    if any(environment.get(name) for name in ('CODEX_HOME', 'OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CODEX_API_KEY')):
        raise ValueError('Native acceptance requires saved configuration without auth or routing environment overrides')
    import tomllib  # This optional native scenario requires the configured Python 3.11+ runtime.
    profile = request.get('codex_profile')
    config = helper.plain(codex_config_path(Path.home(), profile))
    if config.stat().st_size > 4 * 1024 * 1024:
        raise ValueError('Native configuration exceeds evidence bound')
    endpoint = codex_gateway(tomllib.loads(config.read_text(encoding='utf-8-sig')), profile)
    binary = helper.plain(Path(environment['HIROUTE_WORKER_CODEX_BINARY']))
    checksum = helper.digest(binary)
    version = subprocess.run([str(binary), '--version'], stdin=subprocess.DEVNULL,
        capture_output=True, timeout=15, env=environment, creationflags=0x08000000)
    (run / 'codex-version.stdout').write_bytes(version.stdout)
    (run / 'codex-version.stderr').write_bytes(version.stderr)
    if version.returncode or not version.stdout.strip() or len(version.stdout) > 1024:
        raise RuntimeError('The configured native client version could not be measured')
    identity = dict(binary_sha256=checksum, client_version=version.stdout.decode('utf-8-sig').strip(),
                    profile=profile, gateway_endpoint=endpoint)
    (run / 'codex-input.json').write_text(json.dumps(identity, indent=2), encoding='utf-8')
    expected = 'HiRoute-Windows-' + request['run_id'] + ':ok'
    inference_only = request.get('desktop_action') == 'codex-inference'
    prompt = (('Do not use any tools or read or modify files. Reply with exactly ' if inference_only else
               'Do not read or modify any files or change directories. Use your shell tool to run '
               'the read-only PowerShell command Get-Location in the current working directory. '
               'Then reply with exactly ') + expected)
    command = [str(binary), 'exec', '--json', '--skip-git-repo-check', '--sandbox', 'read-only',
               '--config', 'approval_policy="never"']
    if profile:
        command += ['--profile', profile]
    command.append(prompt)
    done = codex_process(run, command, environment)
    evidence = codex_call_evidence(done.stdout, expected, run.name)
    evidence.update(identity, exit_code=done.returncode,
                    scope='inference_only' if inference_only else 'tool_and_inference',
                    native_entry='configured Codex exec; read-only sandbox')
    filename = 'codex-inference.json' if inference_only else 'codex-call.json'
    (run / filename).write_text(json.dumps(evidence, indent=2), encoding='utf-8')
    required = ('turn_completed', 'answer') if inference_only else ('turn_completed', 'answer', 'shell_tool')
    if (helper.digest(binary) != checksum or done.returncode != 0
            or not all(evidence[key] for key in required)):
        if inference_only:
            raise RuntimeError('Native Codex inference did not complete the turn and exact answer checks; inspect private evidence')
        raise RuntimeError('Native Codex call did not complete the real tool and answer checks; inspect private evidence')
    return evidence


def prepared_installer_replacement(request, pm, helper, activity, manifest, root):
    if (not re.fullmatch(r'[a-f0-9]{32}', request.get('observation_run') or '')
            or not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}', request.get('from_product') or '')):
        raise ValueError('Installer resume requires explicit inspection and prior product identities')
    observed = root / 'runs' / request['observation_run']
    before = json.loads((observed / 'result.json').read_text(encoding='utf-8'))
    old_request = json.loads((observed / 'request.json').read_text(encoding='utf-8'))
    if (before.get('passed') is not True or before.get('assessment') != 'inspection_only'
            or old_request['product'] != request['product']
            or old_request['config']['session_id'] != request['config']['session_id']
            or time.time() - (observed / 'result.json').stat().st_mtime > 600):
        raise ValueError('Installer resume requires a recent matching installation inspection')
    facts = helper.inspect_installation(Path.home(), Path(os.environ['LOCALAPPDATA']))
    for name in ('marker', 'install-pending.json'):
        if facts.get(name, {}).get('sha256') != before['installation'].get(name, {}).get('sha256'):
            raise ValueError('Installation changed after inspection')
    pending = facts.get('install-pending.json', {}).get('record', {})
    if pending.get('schema') != 'hiroute.windows-shell-transaction/v1' or pending.get('phase') != 'prepared':
        raise ValueError('Only the exact prepared shell installation can be resumed')
    prior = manifest(root / 'bundles' / request['from_product'])
    marker = facts['marker']['record']
    previous = helper.plain(Path(marker['install_root']))
    if (previous != Path(pending.get('previous_package') or '') or not previous.name.endswith(prior['source_revision'][:12])
            or previous.parent != Path.home() / '.local/lib/hiroute'):
        raise ValueError('The retained previous package does not match the requested product')
    for name in ('hiroute.exe', 'hirouted.exe'):
        if helper.digest(previous / name) != prior['files'][name]['sha256']:
            raise ValueError('The retained previous product changed')
    programs = helper.plain(Path(facts['programs']['path']))
    shell = helper.plain(programs / 'hiroute-shell.exe')
    if (helper.digest(shell) != pm['shell_sha256'] or pending.get('new', {}).get('hiroute-shell.exe') != pm['shell_sha256']
            or any(Path(p.get('ExecutablePath') or '') == shell for p in activity.get('processes', []))):
        raise ValueError('The measured replacement shell must be present and stopped')
    for name in ('hiroute.exe', 'hirouted.exe'):
        path = helper.plain(programs / 'standalone' / name)
        if helper.digest(path) != pm['files'][name]['sha256']:
            raise ValueError('The replacement product changed')
    return facts, programs, shell


def quarantine_generated_uninstaller(programs, state, run, expected, pending, helper):
    if (not re.fullmatch(r'[a-f0-9]{64}', expected or '')
            or 'uninstall.exe' in pending.get('new', {}) or not pending.get('old', {}).get('uninstall.exe')):
        raise ValueError('Only an unregistered generated uninstaller can be quarantined manually')
    source = helper.plain(programs / 'uninstall.exe')
    backup = helper.plain(state / 'install-backups/uninstall.exe')
    if (helper.digest(source) != expected or helper.digest(backup) != pending['old']['uninstall.exe']
            or expected == pending['old']['uninstall.exe']):
        raise ValueError('Generated-file or retained-backup identity changed; no file was moved')
    destination = run / 'unregistered-uninstaller.bin'
    if destination.exists():
        raise ValueError('Quarantine destination already exists')
    source.rename(destination)
    if helper.digest(destination) != expected:
        if not source.exists():
            destination.rename(source)
        raise ValueError('Quarantined generated file changed')
    return dict(native_entry='manual quarantine of an independently verified generated file; not installation acceptance',
                sha256=expected, previous_backup_preserved=True, business_data_untouched=True)


def installer_resume(request, run, pm, helper, activity, manifest, root):
    facts, programs, shell = prepared_installer_replacement(request, pm, helper, activity, manifest, root)
    pending = facts['install-pending.json']['record']
    command = [str(shell), '--installer-install']
    if pending.get('previous_autostart') is False:
        command.append('--no-login-start')
    if facts.get('shell-install.json', {}).get('record', {}).get('tray_enabled') is False:
        command.append('--no-tray')
    done = subprocess.run(command, stdin=subprocess.DEVNULL,
                          capture_output=True, timeout=180, creationflags=0x08000000)
    (run / 'installer-resume.stdout').write_bytes(done.stdout)
    (run / 'installer-resume.stderr').write_bytes(done.stderr)
    (run / 'installer-resume.after.json').write_text(json.dumps(
        helper.inspect_installation(Path.home(), Path(os.environ['LOCALAPPDATA'])),indent=2),encoding='utf-8')
    if done.returncode:
        raise RuntimeError('The real post-install phase failed; inspect installer-resume.stdout before any further action')
    return dict(native_entry='measured installer post-install phase; not a full NSIS result', exit_code=done.returncode)


def probe_tls(run, target):
    import hashlib
    import socket
    import ssl
    host, port = target.get('authority', ''), target.get('port')
    if (target.get('scheme') != 'https' or not re.fullmatch(r'[A-Za-z0-9.-]{1,253}', host)
            or not isinstance(port, int) or not 1 <= port <= 65535):
        raise ValueError('TLS diagnostics require a saved HTTPS DNS target')
    context = ssl.create_default_context()
    context.set_alpn_protocols(['h2', 'http/1.1'])
    result = dict(host=host, port=port, certificate_store=context.cert_store_stats(),
                  authenticated=False, http_request_sent=False, tls_verified=False)
    try:
        with socket.create_connection((host, port), timeout=8) as stream:
            with context.wrap_socket(stream, server_hostname=host) as secured:
                result.update(tls_verified=True, tls_version=secured.version(),
                    alpn=secured.selected_alpn_protocol(),
                    peer_certificate_sha256=hashlib.sha256(secured.getpeercert(binary_form=True)).hexdigest())
    except (OSError, ssl.SSLError) as error:
        result.update(error_type=type(error).__name__, error=str(error)[:1024],
                      verify_code=getattr(error, 'verify_code', None))
    (run / 'tls-probe.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    if not result['tls_verified']:
        raise RuntimeError('TLS verification failed before any authenticated or HTTP request; inspect tls-probe.json')
    return result


def probe_windows_tls(run, target):
    if (target.get('scheme') != 'https'
            or not re.fullmatch(r'[A-Za-z0-9.-]{1,253}', target.get('authority', ''))
            or not isinstance(target.get('port'), int) or not 1 <= target['port'] <= 65535):
        raise ValueError('TLS diagnostics require a saved HTTPS DNS target')
    (run / 'tls-target.json').write_text(json.dumps(target), encoding='utf-8')
    script = run / 'tls-native.ps1'
    script.write_text(r'''
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$q=Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'tls-target.json') | ConvertFrom-Json
$tcp=[Net.Sockets.TcpClient]::new();$ssl=$null
$result=@{tls_verified=$false;authenticated=$false;http_request_sent=$false;stack='Windows SslStream'}
try {
 $connect=$tcp.ConnectAsync([string]$q.authority,[int]$q.port)
 if(-not $connect.Wait(8000)){throw 'TCP connect timeout'}
 $connect.GetAwaiter().GetResult() | Out-Null
 $ssl=[Net.Security.SslStream]::new($tcp.GetStream(),$false)
 $handshake=$ssl.AuthenticateAsClientAsync([string]$q.authority)
 if(-not $handshake.Wait(8000)){throw 'TLS handshake timeout'}
 $handshake.GetAwaiter().GetResult() | Out-Null
 $result.tls_verified=$true;$result.tls_version=$ssl.SslProtocol.ToString()
} catch {$result.error=$_.Exception.GetBaseException().Message}
finally {if($null -ne $ssl){$ssl.Dispose()};$tcp.Dispose()}
$result | ConvertTo-Json -Compress
''', encoding='utf-8-sig')
    done = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-File', str(script)],
        capture_output=True, timeout=30, creationflags=0x08000000)
    (run / 'tls-native.stderr').write_bytes(done.stderr)
    done.check_returncode()
    result = json.loads(done.stdout.decode('utf-8-sig'))
    (run / 'tls-probe.native.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    if not result.get('tls_verified'):
        raise RuntimeError('Windows system TLS verification failed; inspect tls-probe.native.json')
    return result


def network_probe(request, run, roots):
    done = subprocess.run([str(roots['stable_cli']), 'compute', 'list', '--output', 'json'],
        capture_output=True, timeout=30, creationflags=0x08000000)
    done.check_returncode()
    envelope = json.loads(done.stdout)
    matches = [s for s in envelope.get('data', {}).get('sources', [])
               if s.get('source_id') == request.get('source_id') and s.get('state') == 'ready']
    if envelope.get('status') != 'succeeded' or len(matches) != 1:
        raise ValueError('Select one ready saved source for unauthenticated TLS diagnosis')
    probe = probe_windows_tls if request.get('native_tls') else probe_tls
    return probe(run, matches[0]['target'])


def admit_nsis_install(request, helper, activity, manifest, root):
    facts = helper.inspect_installation(Path.home(), Path(os.environ['LOCALAPPDATA']))
    previous = request.get('from_product')
    interrupted = request.get('recover_product')
    if interrupted:
        if not previous or not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}', interrupted):
            raise ValueError('Prepared recovery requires explicit previous and interrupted products')
        prepared_installer_replacement({**request, 'product': interrupted},
            manifest(root / 'bundles' / interrupted), helper, activity, manifest, root)
    elif previous:
        old = root / 'bundles' / previous
        _, _, shell = installed(helper, old, manifest(old))
        if any(Path(p.get('ExecutablePath') or '') == shell for p in activity.get('processes', [])):
            raise ValueError('Quit the old Desktop through its native UI before upgrade')
    elif 'marker' in facts or facts['programs']['exists'] or facts['stable_cli']['exists']:
        raise ValueError('Fresh Desktop installation must not adopt an existing product')



# The shell's installer adapter permits one 600-second native install request.
# NSIS also extracts files and runs its prepare/commit hooks outside that request.
# Keep this specific whole-installer watchdog outside that product deadline;
# ordinary public CLI/UI actions retain their existing bounds.
NSIS_INSTALL_TIMEOUT = 720


def nsis_install(installer, run, locations):
    """Wait for the actual NSIS child and retain bounded phase/timing evidence.

    Observing a committed marker is never a substitute for installer exit. On
    timeout stop only our owned NSIS process; its product transaction remains
    available for inspection and normal recovery, without automatic replay.
    """
    observed = {
        'shell': locations['shell_state'] / 'install-pending.json',
        'standalone': locations['data'] / 'service/install-journal.json',
        'installation': locations['data'] / 'standalone.json',
        'outcome': locations['shell_state'] / 'update-outcome.json',
    }
    started = time.monotonic()
    evidence = dict(watchdog_seconds=NSIS_INSTALL_TIMEOUT, events=[])
    signatures = {}

    def record(final=None):
        changed = {}
        for name, path in observed.items():
            try:
                info = path.stat()
                signature = (info.st_mtime_ns, info.st_size)
                if signature == signatures.get(name):
                    continue
                signatures[name] = signature
                value = dict(present=True, bytes=info.st_size)
                if info.st_size <= 1024 * 1024:
                    with path.open('rb') as source:
                        data = source.read(1024 * 1024 + 1)
                    try:
                        row = json.loads(data)
                        value.update({k: row[k] for k in ('phase', 'status')
                                      if isinstance(row.get(k), str) and len(row[k]) <= 64})
                    except (ValueError, TypeError):
                        value['incomplete_read'] = True
                changed[name] = value
            except FileNotFoundError:
                if signatures.get(name) != 'absent':
                    signatures[name] = 'absent'
                    changed[name] = dict(present=False)
            except OSError:
                if signatures.get(name) != 'unavailable':
                    signatures[name] = 'unavailable'
                    changed[name] = dict(unavailable=True)
        if changed and len(evidence['events']) < 256:
            evidence['events'].append(dict(elapsed_seconds=round(time.monotonic()-started, 3),
                                           changes=changed))
        if final is not None:
            evidence.update(final, elapsed_seconds=round(time.monotonic()-started, 3))
        (run / 'installer-process.json').write_text(json.dumps(evidence, indent=2), encoding='utf-8')

    with (run / 'installer.stdout').open('xb') as out, (run / 'installer.stderr').open('xb') as err:
        child = subprocess.Popen([str(installer), '/S'], stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        evidence['pid'] = child.pid
        while True:
            code = child.poll()
            record()
            if code is not None:
                record(dict(exit_code=code, timed_out=False))
                if code:
                    raise subprocess.CalledProcessError(code, [str(installer), '/S'])
                return evidence
            if time.monotonic()-started >= NSIS_INSTALL_TIMEOUT:
                record(dict(timed_out=True))
                child.kill()
                child.wait(timeout=30)
                raise subprocess.TimeoutExpired([str(installer), '/S'], NSIS_INSTALL_TIMEOUT)
            time.sleep(0.5)


def robocopy_path(path):
    # Robocopy handles long children natively but treats Python's extended
    # device prefix as a network path. Keep that prefix for Python file IO only.
    value = str(path)
    if value.startswith('\\\\?\\UNC\\'):
        return '\\\\' + value[8:]
    return value[4:] if value.startswith('\\\\?\\') else value


def copy_recovery_tree(helper, source, target, log):
    """Offline operator copy, including DACLs and long paths; never mirror/delete."""
    source, target = helper.plain(source), helper.plain(target)
    if not source.is_dir() or target.exists() or source == target or source in target.parents:
        raise ValueError('Recovery copies require a distinct absent destination')
    expected = helper.inventory(source)
    with log.open('wb') as stream:
        done = subprocess.run(['robocopy.exe', robocopy_path(source), robocopy_path(target), '/E', '/COPY:DATS',
            '/DCOPY:DAT', '/R:0', '/W:0', '/XJ', '/NFL', '/NDL', '/NJH', '/NJS', '/NP'],
            stdout=stream, stderr=subprocess.STDOUT, timeout=120, creationflags=0x08000000)
    if done.returncode >= 8 or not target.is_dir() or helper.inventory(target) != expected:
        raise ValueError('Offline recovery copy failed verification; preserve both directories: ' + log.stem)
    return expected


def copy_recovery_file(helper, source, target, directory):
    """Restore one admitted native entry under its recorded name with its DACL."""
    source, target = helper.plain(source), helper.plain(target)
    directory.mkdir()
    if target.exists():
        target.rename(directory/'preserved-current')
    staged = directory/'staged';staged.mkdir()
    done = subprocess.run(['robocopy.exe',robocopy_path(source.parent),str(staged),source.name,
        '/COPY:DATS','/R:0','/W:0','/NFL','/NDL','/NJH','/NJS','/NP'],
        capture_output=True,timeout=30,creationflags=0x08000000)
    copy = staged/source.name
    if done.returncode >= 8 or not copy.is_file() or helper.digest(copy) != helper.digest(source):
        raise ValueError('The admitted original native entry could not be restored; preserve partial recovery')
    target.parent.mkdir(parents=True,exist_ok=True)
    copy.rename(target)


def restore_original(request, run, product, pm, helper, activity, manifest, root):
    """Execute the documented whole-group operator recovery, never an automatic retry."""
    previous, observation = request.get('from_product'), request.get('observation_run')
    if not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}', previous or '') or not re.fullmatch(r'[a-f0-9]{32}', observation or ''):
        raise ValueError('Original restoration requires exact products and a recent successful source preview')
    preview = root / 'runs' / observation
    original = helper.bounded_json(preview / 'request.json')
    result = helper.bounded_json(preview / 'result.json')
    if (original.get('desktop_action') != 'recovery-preview' or original['product'] != request['product']
            or original.get('from_product') != previous or not result.get('passed')
            or result.get('administrator') is not False
            or original['config']['user'] != request['config']['user']
            or original['config']['session_id'] != request['config']['session_id']
            or time.time() - (preview / 'result.json').stat().st_mtime > 3600):
        raise ValueError('Original restoration preview is stale or belongs to another product/account')
    current = root / 'bundles' / previous
    current_manifest = manifest(current)
    if current_manifest['source_revision'] == pm['source_revision']:
        raise ValueError('Whole-group restoration requires distinct current and original programs')
    roots, facts, shell = installed(helper, current, current_manifest)
    source = helper.original_recovery_source(Path.home(), Path(os.environ['LOCALAPPDATA']), pm)
    locations = helper.original_desktop_locations(source,pm,roots)
    measured = helper.bounded_json(preview / 'recovery-source.json')
    if (measured.get('batch_id') != source['batch_id']
            or measured.get('locator_sha256') != source['locator_sha256']):
        raise ValueError('The original recovery source changed after preview')
    marker = facts['marker']['record']
    definition = json.loads(facts['definition']['text'])
    task = definition.get('name', '')
    if not re.fullmatch(r'HiRoute-[a-f0-9]{64}', task) or definition.get('manager') != 'task':
        raise ValueError('Original restoration requires the exact owned Task definition')
    if locations is not None and locations['task_name'] != task:
        raise ValueError('The original Task belongs to another account')
    install = helper.plain(marker['install_root'])
    allowed = {install / 'hirouted.exe', install / 'libexec/cliproxyapi.exe'}
    if (any(Path(p.get('ExecutablePath') or '') not in allowed for p in activity.get('processes', []))
            or any(t.get('TaskName') != task for t in activity.get('tasks', []))):
        raise ValueError('Quit the measured Desktop and preserve unrelated product activity before restoration')
    extras = source['record'].get('extra_locations', {})
    for name, target in extras.items():
        target = helper.plain(target)
        if (name not in ('gateway.lkg', 'gateway-listener.json') or target.name != name
                or helper.native_directory_identity(target.parent) != helper.native_directory_identity(roots['state'])):
            raise ValueError('An original auxiliary destination is outside this native state root')
    newer = run / 'preserved-current'
    newer.mkdir()
    stages = []
    def stage(name):
        stages.append(name)
        (run / 'recovery-operator.json').write_text(json.dumps(dict(batch_id=source['batch_id'],
            old_revision=pm['source_revision'], current_revision=manifest(current)['source_revision'],
            stages=stages, preserved_current=str(newer)), indent=2), encoding='utf-8')
    def command(program, args, label):
        done = subprocess.run([str(program), *args], capture_output=True, timeout=150, creationflags=0x08000000)
        (run / (label + '.stdout')).write_bytes(done.stdout)
        (run / (label + '.stderr')).write_bytes(done.stderr)
        if done.returncode:
            raise ValueError('The owned recovery command failed: ' + label)
        return done
    native_before = {str(p): helper.digest(p) for p in (Path.home()/'.codex/config.toml', Path.home()/'.codex/hiroute.config.toml') if p.is_file()}
    import winreg
    run_key = r'Software\Microsoft\Windows\CurrentVersion\Run'
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, run_key, 0, winreg.KEY_QUERY_VALUE | winreg.KEY_SET_VALUE) as key:
        try:
            login, kind = winreg.QueryValueEx(key, 'HiRoute')
        except FileNotFoundError:
            login, kind = None, None
        if login is not None and (kind != winreg.REG_SZ or login != '"' + str(shell) + '" --background'):
            raise ValueError('The current-user HiRoute login value belongs to another installation')
        (newer / 'shell-login.json').write_text(json.dumps(dict(value=login, kind=kind)), encoding='utf-8')
        command('reg.exe', ['export', r'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\HiRoute', str(newer/'uninstall.reg'), '/y'], 'preserve-registration')
        command('schtasks.exe', ['/Query', '/TN', task, '/XML'], 'preserve-task')
        if login is not None:
            winreg.DeleteValue(key, 'HiRoute')
    stage('preserved-registration-and-disabled-owned-shell-login')
    command(install/'hiroute.exe', ['service','autostart','disable','--output','json'], 'disable-current-task-login')
    command(install/'hiroute.exe', ['service','stop','--output','json'], 'stop-current')
    images = ','.join("'" + str(p).replace("'", "''") + "'" for p in allowed | {shell})
    check = subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',
        "$ErrorActionPreference='Stop';$images=@(" + images + ");@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -in $images}).Count"],
        capture_output=True, timeout=30, text=True, creationflags=0x08000000)
    if check.returncode or check.stdout.strip() != '0':
        raise ValueError('Owned processes remain; preserve the stopped recovery state before copying data')
    stage('current-task-stopped-login-disabled-and-processes-exited')
    if locations is not None:
        # Follow the guide's ordering: remove the stopped registration before
        # activating old data, so a pending failure restart cannot launch the
        # newer daemon against that restored group.
        command('schtasks.exe',['/Delete','/TN',task,'/F'],'remove-stopped-current-task')
        stopped_again=subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',
            "$ErrorActionPreference='Stop';$images=@("+images+");@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -in $images}).Count"],
            capture_output=True,timeout=30,text=True,creationflags=0x08000000)
        if stopped_again.returncode or stopped_again.stdout.strip()!='0':
            raise ValueError('An owned process raced Task removal; no old data was activated')
        stage('stopped-current-registration-removed-before-old-data-activation')
    for label, path in [('program',install),('desktop',roots['programs']),('installation',roots['data'])]:
        copy_recovery_tree(helper, path, newer/label, run/(label+'-copy.log'))
    storage = helper.plain(helper.extended_windows_path(str(roots['state']/'storage')))
    replacement = storage.with_name('storage.original-restore.' + request['run_id'])
    expected = copy_recovery_tree(helper, source['source']/'storage', replacement, run/'storage-copy.log')
    stage('verified-complete-old-group-offline-copy')
    storage.rename(newer/'storage')
    replacement.rename(storage)
    auxiliary = newer/'auxiliary';auxiliary.mkdir()
    for name, target in extras.items():
        target = helper.plain(target)
        if target.exists():target.rename(auxiliary/name)
        done = subprocess.run(['robocopy.exe',robocopy_path(source['source']),robocopy_path(target.parent),name,'/COPY:DATS','/R:0','/W:0','/NFL','/NDL','/NJH','/NJS','/NP'],
            capture_output=True, timeout=30, creationflags=0x08000000)
        if done.returncode >= 8 or helper.digest(target) != helper.digest(source['source']/name):
            raise ValueError('Original auxiliary restoration failed; do not start either program')
    if helper.inventory(storage) != expected:
        raise ValueError('The complete restored original group changed before program replacement')
    stage('complete-old-group-and-auxiliary-files-restored')
    timing = None
    restored_choices = None
    if locations is None:
        # The legacy source has no complete Desktop registration. Use its exact
        # independently verified original NSIS only after pairing the old group.
        timing = nsis_install(product/'installer.exe',run,roots)
        stage('matching-original-native-installer-completed')
    else:
        package = source['source']/'recovery-package'
        for label,old,target in [('program',package/'program',Path(locations['program'])),
                                 ('desktop',package/'desktop',Path(locations['desktop']))]:
            target = helper.plain(target)
            if target.exists():target.rename(newer/('displaced-'+label))
            copy_recovery_tree(helper,old,target,run/('restore-'+label+'.log'))
        for index,(name,target) in enumerate(sorted(locations['files'].items())):
            copy_recovery_file(helper,package/name,Path(target),newer/('native-entry-'+str(index)))
        command('reg.exe',['import',robocopy_path(package/'installation/uninstall.reg')],'restore-registration')
        command('schtasks.exe',['/Create','/TN',task,'/XML',robocopy_path(package/'installation/task.xml')],'restore-original-task')
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,run_key,0,winreg.KEY_QUERY_VALUE|winreg.KEY_SET_VALUE) as key:
            try:winreg.QueryValueEx(key,'HiRoute')
            except FileNotFoundError:pass
            else:raise ValueError('A login registration appeared during recovery; preserve it')
            if locations['shell_login']:
                winreg.SetValueEx(key,'HiRoute',0,winreg.REG_SZ,'"'+str(shell)+'" --background')
        command(roots['stable_cli'],['service','start','--output','json'],'start-original-task')
        command(roots['stable_cli'],['service','autostart','enable' if locations['task_autostart'] else 'disable','--output','json'],'restore-original-task-login')
        state=command(roots['stable_cli'],['service','status','--output','json'],'restored-login-status')
        task_login=json.loads(state.stdout).get('data',{}).get('autostart_enabled')
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,run_key,0,winreg.KEY_QUERY_VALUE) as key:
            try:value,kind=winreg.QueryValueEx(key,'HiRoute')
            except FileNotFoundError:value,kind=None,None
        expected_login='"'+str(shell)+'" --background' if locations['shell_login'] else None
        if (task_login!=locations['task_autostart'] or value!=expected_login
                or value is not None and kind!=winreg.REG_SZ):
            raise ValueError('The original independent Shell and Task login choices were not restored')
        restored_choices=dict(task_autostart=task_login,shell_login=value is not None)
        stage('original-programs-native-entries-apps-and-task-registration-restored')
    installed(helper,product,pm)
    status = command(roots['stable_cli'],['system','status','--output','json'],'restored-system')
    if json.loads(status.stdout).get('data', {}).get('gateway') != 'ready':
        raise ValueError('The restored original Gateway is not ready; preserve the recovery state')
    if any(helper.digest(Path(p)) != checksum for p,checksum in native_before.items()):
        raise ValueError('Native Agent configuration changed during whole-group restoration')
    stage('original-control-readiness-and-native-config-verified')
    return dict(native_entry='documented complete-group operator recovery with ' + ('exact original NSIS' if locations is None else 'retained complete Desktop and native Task XML'),
                batch_id=source['batch_id'], stages=stages, installer=timing, preserved_current=str(newer),
                restored_login_choices=restored_choices,
                scope='legacy retained source; original Desktop preferences were not captured' if locations is None else 'complete original Desktop, program, native files and independent login choices')


def failure_page_languages(run, shell, pid, cause, source=None):
    import shutil
    observed=ui_action(run,dict(desktop_action='snapshot'),shell,pid)
    deadline=time.monotonic()+75
    languages=[]
    reasons={'locked':('另一个 HiRoute 实例','Another HiRoute instance'),
             'unsupported_format':('这组数据的格式没有经过验证','This data format has no verified upgrade path')}
    for index in range(2):
        while True:
            names='\n'.join(n.get('name','') for n in observed.get('nodes',[]))
            zh,en=reasons[cause]
            language='zh' if zh in names else 'en' if en in names else None
            if language is not None:break
            if time.monotonic()>=deadline:raise ValueError('The native failure page did not expose the current cause')
            time.sleep(1)
            observed=ui_action(run,dict(desktop_action='snapshot'),shell,pid)
        prefix='locked' if cause=='locked' else 'format'
        shutil.copyfile(run/'window.png',run/(prefix+'-'+language+'.png'))
        shutil.copyfile(run/'ui.json',run/(prefix+'-'+language+'.json'))
        if source is not None and (source['batch_id'] not in names or source['batch'].name not in names or 'RECOVERY.md' not in names):
            raise ValueError('The actual failure page did not locate the exact verified original batch and guide')
        languages.append(language)
        if index==0:
            buttons=[n for n in observed['nodes'] if n.get('name') in ('English','中文') and n.get('type')=='ControlType.Button' and not n.get('offscreen')]
            if len(buttons)!=1:raise ValueError('The actual failure-page language control is unavailable')
            observed=ui_action(run,dict(desktop_action='invoke'),shell,pid,buttons[0]['runtime_id'])
    if set(languages)!={'zh','en'}:raise ValueError('Both native failure-page languages were not observed')
    return languages


def startup_ui_context(request, product, pm, helper, activity, root):
    observation = request.get('observation_run')
    if not re.fullmatch(r'[a-f0-9]{32}',observation or ''):
        raise ValueError('The native lock UI case requires a recent current-product inspection')
    inspected = root/'runs'/observation
    before_request = helper.bounded_json(inspected/'request.json')
    before_result = helper.bounded_json(inspected/'result.json')
    if (before_request.get('operation') != 'inspect' or before_request['product'] != request['product']
            or not before_result.get('passed') or before_result.get('administrator') is not False
            or before_request['config']['user'] != request['config']['user']
            or before_request['config']['session_id'] != request['config']['session_id']
            or time.time()-(inspected/'result.json').stat().st_mtime>3600):
        raise ValueError('The native lock UI inspection is stale or belongs to another product/account')
    roots,facts,shell = installed(helper,product,pm)
    if any(Path(p.get('ExecutablePath') or '') == shell for p in activity.get('processes', [])):
        raise ValueError('Quit the installed Desktop before the bounded native UI case')
    install=Path(facts['marker']['record']['install_root'])
    allowed={install/'hirouted.exe',install/'libexec/cliproxyapi.exe'}
    task=json.loads(facts['definition']['text']).get('name','')
    if (not re.fullmatch(r'HiRoute-[a-f0-9]{64}',task)
            or any(Path(p.get('ExecutablePath') or '') not in allowed for p in activity.get('processes',[]))
            or any(t.get('TaskName')!=task for t in activity.get('tasks',[]))):
        raise ValueError('Preserve unrelated product activity before the bounded native UI fixture')
    if not desktop_state().get('interactive'):
        raise ValueError('The existing input desktop is unavailable; no lock fixture was created')
    return roots,facts,shell


def locked_startup_ui(request, run, product, pm, helper, activity, root, baseline=None):
    """Own one bounded lock fixture, inspect both real languages, then reopen normally."""
    import shutil
    roots,facts,shell = startup_ui_context(request,product,pm,helper,activity,root)
    if baseline is not None:
        source = helper.original_recovery_source(Path.home(),Path(os.environ['LOCALAPPDATA']),baseline)
        if helper.original_desktop_locations(source,baseline,roots) is None:
            raise ValueError('The file-use case requires a complete retained Desktop source')
    cli = Path(facts['marker']['record']['install_root'])/'hiroute.exe'
    def command(args,label):
        done=subprocess.run([str(cli),*args],capture_output=True,timeout=150,creationflags=0x08000000)
        (run/(label+'.stdout')).write_bytes(done.stdout);(run/(label+'.stderr')).write_bytes(done.stderr)
        if done.returncode:raise ValueError('The current-product fixture command failed: '+label)
        return json.loads(done.stdout)
    if command(['system','status','--output','json'],'before-system').get('data',{}).get('gateway') != 'ready':
        raise ValueError('The current Gateway must be healthy before the native lock fixture')
    command(['service','stop','--output','json'],'fixture-stop')
    storage=roots['state']/'storage'
    report=helper.plain(storage/'startup-recovery.json')
    report_before=helper.digest(report) if report.exists() else None
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    from ctypes import wintypes
    kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
    kernel.CreateFileW.restype=wintypes.HANDLE;kernel.CloseHandle.argtypes=[wintypes.HANDLE]
    target=storage/('live/control.db' if baseline is not None else 'startup.lock')
    handle=kernel.CreateFileW(str(helper.plain(target)),0,0,None,3,0x00200000,None)
    if handle==wintypes.HANDLE(-1).value:raise ctypes.WinError(ctypes.get_last_error())
    child=None
    languages=[]
    failure=None
    try:
        child=subprocess.Popen([str(shell)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        time.sleep(15)
        languages=failure_page_languages(run,shell,child.pid,'locked')
        offline=command(['service','recovery','--output','json'],'locked-offline-recovery').get('data',{}).get('recovery',{})
        if offline.get('cause')!='locked' or offline.get('action')!='retry_current' or offline.get('backup_batch'):
            raise ValueError('A correctable native file-use prerequisite did not offer current-version retry')
    except Exception as error:
        failure=error
    finally:
        kernel.CloseHandle(handle)
        if child is not None and child.poll() is None:
            try:
                ui_action(run,dict(desktop_action='quit'),shell,child.pid)
                child.wait(timeout=20)
            except Exception:
                if child.poll() is None:child.terminate();child.wait(timeout=20)
    recovery_file=helper.plain(storage/'startup-recovery.json')
    unchanged=(helper.digest(recovery_file) if recovery_file.exists() else None)==report_before
    if baseline is None and not unchanged and failure is None:
        failure=ValueError('A non-writer lock failure overwrote the previous recovery report')
    # Follow the visible instruction: release the file use and reopen this same
    # complete version. Keep business readiness separate from the window result.
    reopened=subprocess.Popen([str(shell)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        time.sleep(15)
        ui_action(run,dict(desktop_action='snapshot'),shell,reopened.pid)
        shutil.copyfile(run/'window.png',run/'reopened-current.png')
        if command(['system','status','--output','json'],'reopened-system').get('data',{}).get('gateway')!='ready':
            raise ValueError('Reopening the current version did not restore actual Gateway readiness')
    finally:
        if reopened.poll() is None:
            try:ui_action(run,dict(desktop_action='quit'),shell,reopened.pid);reopened.wait(timeout=20)
            except Exception:
                if reopened.poll() is None:reopened.terminate();reopened.wait(timeout=20)
    if failure is not None:raise failure
    return dict(native_entry='real standard-user '+('current-Control file-use' if baseline is not None else 'locked-startup')+' page and current-version reopen',languages=languages,
                recovery_report_unchanged=unchanged,gateway_ready_after_reopen=True)


def restore_control_fixture(helper, storage, before, original_header):
    """Undo only the fixture's four bytes, refusing any unexpected business write."""
    control=helper.plain(storage/'live/control.db')
    changed=str(control.relative_to(storage))
    ignored={changed,'startup.lock','startup-recovery.json'}
    after=helper.inventory(storage)
    if any(before.get(name)!=after.get(name) for name in set(before)|set(after) if name not in ignored):
        raise ValueError('Unexpected business changes; preserve both current copies and inspect before recovery')
    with control.open('r+b') as file:
        header=file.read(100)
        if (len(header)!=100 or header[:60]!=original_header[:60] or header[64:]!=original_header[64:]
                or header[60:64]!=(2147483647).to_bytes(4,'big')):
            raise ValueError('The controlled format fixture changed; no bytes were restored')
        check=hashlib.sha256(original_header)
        for block in iter(lambda:file.read(1024*1024),b''):check.update(block)
        if check.hexdigest()!=before[changed]['sha256']:
            raise ValueError('Unexpected Control contents; no bytes were restored')
        file.seek(60);file.write(original_header[60:64]);file.flush();os.fsync(file.fileno())


def format_startup_ui(request, run, product, pm, helper, activity, root, baseline):
    """Preserve a clean current group, refuse a future marker, then leave it offline for explicit restore."""
    roots,facts,shell=startup_ui_context(request,product,pm,helper,activity,root)
    source=helper.original_recovery_source(Path.home(),Path(os.environ['LOCALAPPDATA']),baseline)
    locations=helper.original_desktop_locations(source,baseline,roots)
    if locations is None:
        raise ValueError('The format UI case requires a complete retained Desktop and original Task')
    guides={}
    for name in ('RECOVERY.md','恢复说明.md','PROGRAM-LOCATIONS.md'):
        path=helper.plain(source['source']/name)
        if not path.is_file() or not 1<=path.stat().st_size<=128*1024:
            raise ValueError('The original platform/language recovery guide is unavailable')
        text=path.read_text(encoding='utf-8')
        if name!='PROGRAM-LOCATIONS.md' and source['batch_id'] not in text:
            raise ValueError('A generated recovery guide belongs to another batch')
        if name=='PROGRAM-LOCATIONS.md' and ('task.xml' not in text or locations['task_name'] not in text
                or 'service install' in text or 'service uninstall' in text):
            raise ValueError('The original Windows guide is not paired with its native registration')
        (run/name).write_text(text,encoding='utf-8')
        guides[name]=dict(sha256=helper.digest(path),bytes=path.stat().st_size)
    install=Path(facts['marker']['record']['install_root']);cli=install/'hiroute.exe'
    def command(args,label):
        done=subprocess.run([str(cli),*args],capture_output=True,timeout=150,creationflags=0x08000000)
        (run/(label+'.stdout')).write_bytes(done.stdout);(run/(label+'.stderr')).write_bytes(done.stderr)
        if done.returncode:raise ValueError('The format fixture command failed: '+label)
        return json.loads(done.stdout)
    if command(['system','status','--output','json'],'before-system').get('data',{}).get('gateway')!='ready':
        raise ValueError('The current Gateway must be healthy before the controlled format fixture')
    storage=helper.plain(roots['state']/'storage')
    saved=run/'preserved-clean-current';saved.mkdir()
    changed=False;passed=False;child=None;original_header=None;before=None
    command(['service','stop','--output','json'],'fixture-stop')
    try:
        images=','.join("'"+str(p).replace("'","''")+"'" for p in (install/'hirouted.exe',install/'libexec/cliproxyapi.exe',shell))
        stopped=subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',
            "$ErrorActionPreference='Stop';$images=@("+images+");@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -in $images}).Count"],
            capture_output=True,text=True,timeout=30,creationflags=0x08000000)
        if stopped.returncode or stopped.stdout.strip()!='0':raise ValueError('Owned processes remain; no format fixture was written')
        for name,path in [('storage',storage),('program',install),('desktop',roots['programs']),('installation',roots['data'])]:
            copy_recovery_tree(helper,path,saved/name,run/(name+'-preservation.log'))
        before=helper.inventory(saved/'storage',exclude_startup_lock=True)
        control=helper.plain(storage/'live/control.db')
        wal=helper.plain(storage/'live/control.db-wal')
        if wal.exists() and wal.stat().st_size:raise ValueError('A nonempty Control WAL must be preserved and diagnosed before this fixture')
        import msvcrt
        with helper.plain(storage/'startup.lock').open('r+b') as barrier:
            msvcrt.locking(barrier.fileno(),msvcrt.LK_NBLCK,1)
            try:
                if helper.inventory(storage,exclude_startup_lock=True)!=before:
                    raise ValueError('Current data changed after preservation; no format fixture was written')
                with control.open('r+b') as file:
                    original_header=file.read(100)
                    if original_header[:16]!=b'SQLite format 3\0' or int.from_bytes(original_header[60:64],'big')!=29:
                        raise ValueError('The current Control schema is not the admitted format-29 fixture source')
                    changed=True
                    file.seek(60);file.write((2147483647).to_bytes(4,'big'));file.flush();os.fsync(file.fileno())
            finally:
                barrier.seek(0);msvcrt.locking(barrier.fileno(),msvcrt.LK_UNLCK,1)
        child=subprocess.Popen([str(shell)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        time.sleep(15)
        languages=failure_page_languages(run,shell,child.pid,'unsupported_format',source)
        offline=command(['service','recovery','--output','json'],'format-offline-recovery').get('data',{})
        recovery=offline.get('recovery',{})
        if (recovery.get('cause')!='unsupported_format' or recovery.get('action')!='restore_complete_group'
                or recovery.get('backup_batch')!=source['batch_id'] or 'RECOVERY.md' not in offline.get('instructions','')
                or robocopy_path(source['source']).casefold() not in offline.get('instructions','').casefold()):
            raise ValueError('Offline recovery did not identify the exact complete source and guide')
        (run/'recovery-source.json').write_text(json.dumps(dict(batch_id=source['batch_id'],source=str(source['source']),
            locator_sha256=source['locator_sha256'],preserved_clean_current=str(saved)),indent=2),encoding='utf-8')
        passed=True
        return dict(native_entry='real standard-user future-format failure page and offline guide',languages=languages,
                    batch_id=source['batch_id'],preserved_clean_current=str(saved),
                    guide_files=guides,
                    service_state='offline; explicit complete-group operator restoration required')
    finally:
        if child is not None and child.poll() is None:
            try:ui_action(run,dict(desktop_action='quit'),shell,child.pid);child.wait(timeout=20)
            except Exception:
                if child.poll() is None:child.terminate();child.wait(timeout=20)
        if not passed:
            if changed:restore_control_fixture(helper,storage,before,original_header)
            command(['service','start','--output','json'],'fixture-cleanup-start')
            ready=command(['system','status','--output','json'],'fixture-cleanup-system').get('data',{}).get('gateway')=='ready'
            if not ready:raise ValueError('The controlled fixture could not return the current Gateway to readiness')


def prepare_manifest_bytes(value, pm):
    """Admit data for the measured candidate; never accept executable paths or overrides."""
    if (not isinstance(value,dict) or value.get('revision')!=pm['source_revision']
            or value.get('target')!='x86_64-pc-windows-msvc' or not isinstance(value.get('files'),dict)):
        raise ValueError('The preparation manifest is not the measured candidate')
    for name in ('hiroute.exe','hirouted.exe'):
        if value['files'].get('bin/'+name,{}).get('sha256')!=pm['files'][name]['sha256']:
            raise ValueError('The preparation manifest substitutes a candidate binary')
    data=json.dumps(value).encode('utf-8')
    if not 1<=len(data)<=64*1024:raise ValueError('The preparation manifest exceeds its bound')
    return data


def prepare_shell_bytes(value,pm):
    if not isinstance(value,str) or not 1<=len(value)<=48*1024*1024:
        raise ValueError('The actual NSIS preparation shell is unavailable or exceeds its bound')
    try:data=base64.b64decode(value,validate=True)
    except ValueError as error:raise ValueError('The preparation shell encoding is invalid') from error
    if not 1<=len(data)<=32*1024*1024 or hashlib.sha256(data).hexdigest()!=pm.get('shell_sha256'):
        raise ValueError('The preparation shell differs from the measured NSIS member')
    return data


def installer_prepare(request,run,product,pm,helper,activity,manifest,root):
    """Diagnose the real mutating pre-replacement adapter; never claim an NSIS result."""
    previous,observation=request.get('from_product'),request.get('observation_run')
    if not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}',previous or '') or not re.fullmatch(r'[a-f0-9]{32}',observation or ''):
        raise ValueError('Preparation diagnosis requires the exact installed product and recent inspection')
    inspected=root/'runs'/observation
    original=helper.bounded_json(inspected/'request.json');result=helper.bounded_json(inspected/'result.json')
    if (original.get('operation')!='inspect' or original.get('product')!=previous or not result.get('passed')
            or result.get('administrator') is not False or original['config']['user']!=request['config']['user']
            or original['config']['session_id']!=request['config']['session_id']
            or time.time()-(inspected/'result.json').stat().st_mtime>3600):
        raise ValueError('Preparation inspection is stale or belongs to another product/account')
    admit_nsis_install(request,helper,activity,manifest,root)
    facts=helper.inspect_installation(Path.home(),Path(os.environ['LOCALAPPDATA']))
    if facts.get('install-pending.json') or facts.get('install-journal.json'):
        raise ValueError('Preserve the pending native transaction; do not replay preparation diagnosis')
    path=run/'incoming-package.json';path.write_bytes(prepare_manifest_bytes(request.get('prepare_manifest'),pm))
    adapter=run/'candidate-preparation.exe';adapter.write_bytes(prepare_shell_bytes(request.get('prepare_shell'),pm))
    done=subprocess.run([str(adapter),'--installer-prepare',str(path),str(product/'hirouted.exe')],
        capture_output=True,timeout=180,creationflags=0x08000000)
    (run/'prepare-adapter.stdout').write_bytes(done.stdout);(run/'prepare-adapter.stderr').write_bytes(done.stderr)
    (run/'prepare-adapter.after.json').write_text(json.dumps(
        helper.inspect_installation(Path.home(),Path(os.environ['LOCALAPPDATA'])),indent=2),encoding='utf-8')
    if done.returncode:raise ValueError('The native preparation adapter refused; inspect its captured stderr before another action')
    return dict(native_entry='measured NSIS preparation adapter; not a full NSIS result',exit_code=done.returncode,
                assessment='preparation_only; installed programs were not replaced')


def execute(request, run, product, pm, helper, activity, manifest, root):
    if activity.get('administrator_groups') is not False:
        raise ValueError('Native Desktop evidence requires a standard account, not a filtered administrator')
    action = request['desktop_action']
    if action not in ('install', 'installer-prepare', 'installer-resume', 'installer-quarantine', 'launch', 'snapshot', 'invoke', 'text', 'click', 'scroll', 'close', 'quit', 'tray-observe', 'desktop-state', 'cli', 'native-files', 'codex-call', 'codex-inference', 'network-probe', 'recovery-preview', 'recovery-restore-original', 'recovery-lock-ui', 'recovery-file-use-ui', 'recovery-format-ui'):
        raise ValueError('Unknown native Desktop action')
    if action=='installer-prepare':
        return installer_prepare(request,run,product,pm,helper,activity,manifest,root)
    if action == 'recovery-restore-original':
        return restore_original(request, run, product, pm, helper, activity, manifest, root)
    if action == 'recovery-lock-ui':
        return locked_startup_ui(request,run,product,pm,helper,activity,root)
    if action in ('recovery-file-use-ui','recovery-format-ui'):
        previous=request.get('from_product')
        if not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}',previous or ''):
            raise ValueError('The file-use case requires the independently measured original product')
        baseline=manifest(root/'bundles'/previous)
        if action=='recovery-format-ui':return format_startup_ui(request,run,product,pm,helper,activity,root,baseline)
        return locked_startup_ui(request,run,product,pm,helper,activity,root,baseline)
    if action == 'recovery-preview':
        previous = request.get('from_product')
        observation = request.get('observation_run')
        if not re.fullmatch(r'[a-f0-9]{12}-[a-f0-9]{12}', previous or '') or not re.fullmatch(r'[a-f0-9]{32}', observation or ''):
            raise ValueError('Original recovery preview requires the current product and recent inspection')
        inspection = root / 'runs' / observation
        original = json.loads((inspection / 'request.json').read_text(encoding='utf-8'))
        result = json.loads((inspection / 'result.json').read_text(encoding='utf-8'))
        if (original.get('operation') != 'inspect' or original['product'] != previous
                or not result.get('passed') or result.get('administrator') is not False
                or original['config']['user'] != request['config']['user']
                or original['config']['session_id'] != request['config']['session_id']
                or time.time() - (inspection / 'result.json').stat().st_mtime > 3600):
            raise ValueError('The original recovery inspection is stale or belongs to another product/account')
        installed(helper, root / 'bundles' / previous, manifest(root / 'bundles' / previous))
        source = helper.original_recovery_source(Path.home(), Path(os.environ['LOCALAPPDATA']), pm)
        facts = dict(batch_id=source['batch_id'], source=str(source['source']),
                     baseline_revision=pm['source_revision'], copied_materials=len(source['record']['files']),
                     locator_sha256=source['locator_sha256'], extra_locations=source['record'].get('extra_locations'),
                     source_keys=list(source['record']), info_keys=list(source['record'].get('source_info', {})))
        (run / 'recovery-source.json').write_text(json.dumps(facts, indent=2), encoding='utf-8')
        return dict(native_entry='read-only original recovery material verification', **facts)
    if action == 'desktop-state':
        return dict(native_entry='read-only input desktop inspection', state=desktop_state())
    if action == 'native-files':
        files = {}
        for relative in ('.codex/config.toml', '.codex/hiroute.config.toml', '.claude/settings.json',
                         '.agents/skills/hiroute-collaboration', '.claude/skills/hiroute-collaboration',
                         '.agents/skills/hiroute-management', '.claude/skills/hiroute-management'):
            path = helper.plain(Path.home() / relative)
            if not path.exists():
                files[relative] = dict(exists=False)
            elif path.is_file():
                if path.stat().st_size > 4 * 1024 * 1024:
                    raise ValueError('Native configuration exceeds evidence bound')
                name = 'native-' + relative.replace('/', '_').lstrip('.')
                (run / name).write_bytes(path.read_bytes())
                files[relative] = dict(exists=True, sha256=helper.digest(path), bytes=path.stat().st_size)
            else:
                files[relative] = dict(exists=True, inventory=helper.inventory(path))
        # Compare these before/after an upgrade or restoration without invoking
        # Codex in between. Calls themselves legitimately append native history.
        for relative in ('.codex/sessions','.codex/history.jsonl'):
            path=helper.plain(Path.home()/relative)
            if not path.exists():files[relative]=dict(exists=False)
            elif path.is_dir():files[relative]=dict(exists=True,inventory=helper.inventory(path))
            else:files[relative]=dict(exists=True,sha256=helper.digest(path),bytes=path.stat().st_size)
        (run / 'native-files.json').write_text(json.dumps(files, indent=2), encoding='utf-8')
        return dict(native_entry='read-only native configuration inventory', files=files)
    if action == 'installer-resume':
        return installer_resume(request, run, pm, helper, activity, manifest, root)
    if action == 'installer-quarantine':
        facts, programs, _ = prepared_installer_replacement(request, pm, helper, activity, manifest, root)
        return quarantine_generated_uninstaller(programs, Path(facts['shell_state']['path']), run,
            request.get('generated_uninstaller_sha256'), facts['install-pending.json']['record'], helper)
    if action == 'install':
        admit_nsis_install(request, helper, activity, manifest, root)
        previous = request.get('from_product')
        timing = nsis_install(product / 'installer.exe', run,
                              helper.locations(Path.home(), Path(os.environ['LOCALAPPDATA'])))
        _, after, _ = installed(helper, product, pm)
        return dict(native_entry='NSIS /S', exit_code=timing['exit_code'], installation=after,
                    elapsed_seconds=timing['elapsed_seconds'],
                    upgrade_from=previous, recovery_from=request.get('recover_product'),
                    installer_sha256=pm['files']['installer.exe']['sha256'])
    roots, facts, shell = installed(helper, product, pm)
    if action == 'network-probe':
        return network_probe(request, run, roots)
    if action in ('codex-call', 'codex-inference'):
        return codex_call(run, request, helper)
    if action == 'launch':
        child = subprocess.Popen([str(shell)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        # A second launch uses the product's single-instance protocol. Discover
        # its real window, rather than assuming this new PID owns the WebView.
        time.sleep(4)
        return dict(native_entry='installed Desktop', launched_pid=child.pid,
                    shell_sha256=helper.digest(shell))
    if action == 'cli':
        args = request['cli_args']
        if (not isinstance(args, list) or not args or len(args) > 40
                or any(not isinstance(a, str) or len(a) > 64 * 1024 or '\x00' in a for a in args)
                or args[0] not in ('system', 'service', 'agents', 'agent', 'plans', 'routing', 'gateway', 'compute', 'operations', 'console', 'sessions')):
            raise ValueError('Only bounded public product CLI families are accepted')
        environment = os.environ.copy()
        environment.update(request['config']['environment'])
        environment['PATH'] = ';'.join(request['config']['path']) + ';' + environment['PATH']
        done = subprocess.run([str(roots['stable_cli']), *args], capture_output=True, timeout=150,
                              env=environment, creationflags=0x08000000)
        (run / 'cli.stdout').write_bytes(done.stdout)
        (run / 'cli.stderr').write_bytes(done.stderr)
        return dict(native_entry='installed public CLI', exit_code=done.returncode)
    candidates = [p for p in activity.get('processes', []) if Path(p.get('ExecutablePath') or '') == shell]
    if len(candidates) != 1:
        raise ValueError('Exactly one measured installed Desktop process is required')
    pid = candidates[0]['ProcessId']
    element = None
    observed = None
    if action in ('invoke', 'text', 'click', 'scroll'):
        parent = root / 'runs' / request['observation_run']
        old_request = json.loads((parent / 'request.json').read_text(encoding='utf-8'))
        old_result = json.loads((parent / 'result.json').read_text(encoding='utf-8'))
        observed = json.loads((parent / 'ui.json').read_text(encoding='utf-8'))
        if (not old_result.get('passed') or old_request['product'] != request['product']
                or old_request['config']['session_id'] != request['config']['session_id']
                or observed['pid'] != pid or time.time() - (parent / 'ui.json').stat().st_mtime > 600):
            raise ValueError('A recent observation of this exact Desktop process is required')
        if action not in ('click', 'scroll'):
            matches = [e for e in observed['nodes'] if e['runtime_id'] == request['element']]
            if not matches:
                raise ValueError('Choose one observed native UI element')
            element = matches[0]
    return dict(native_entry='Windows UI Automation', observation=ui_action(run, request, shell, pid, element, observed),
                shell_sha256=helper.digest(shell))
