#!/usr/bin/env python3
"""Bounded native Desktop acceptance in an existing validation session.

Uses the installed NSIS product, Windows UI Automation and window screenshots.
It never injects browser scripts, creates a login or changes native data roots.
"""
import json
import os
from pathlib import Path
import subprocess
import time


UI = r'''
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
$q=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'ui-request.json') -Raw | ConvertFrom-Json
$p=Get-Process -Id $q.pid -ErrorAction Stop
if($p.Path -ne $q.image){throw 'Desktop image changed'}
$p.Refresh()
if($p.MainWindowHandle -eq 0){throw 'No visible native Desktop window'}
$root=[Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
$all=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
if($all.Count -gt 1600){throw 'UI Automation tree exceeds observation bound'}
if($q.action -in @('invoke','text','close')) {
 if($q.action -eq 'close') {
  $root.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close()
 } else {
  $matches=@($all | Where-Object {($_.GetRuntimeId() -join '.') -eq $q.element.runtime_id})
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
 $all=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
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
@{pid=$p.Id;handle=$p.MainWindowHandle.ToInt64();window=$root.Current.Name;nodes=$nodes}|ConvertTo-Json -Depth 6 -Compress
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


def ui_action(run, request, shell, pid, element=None):
    query = dict(pid=pid, image=str(shell), action=request['desktop_action'], element=element,
                 text=request.get('text'))
    (run / 'ui-request.json').write_text(json.dumps(query), encoding='utf-8')
    script = run / 'ui.ps1'
    script.write_text(UI, encoding='utf-8-sig')
    completed = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-File', str(script)],
                               capture_output=True, timeout=60)
    (run / 'ui.stderr').write_bytes(completed.stderr)
    if completed.returncode:
        raise RuntimeError('Native UI Automation failed; inspect ui.stderr before retrying')
    observed = json.loads(completed.stdout.decode('utf-8-sig'))
    (run / 'ui.json').write_text(json.dumps(observed, ensure_ascii=False, indent=2), encoding='utf-8')
    return observed


def execute(request, run, product, pm, helper, activity, manifest, root):
    if activity.get('administrator_groups') is not False:
        raise ValueError('Native Desktop evidence requires a standard account, not a filtered administrator')
    action = request['desktop_action']
    if action == 'install':
        facts = helper.inspect_installation(Path.home(), Path(os.environ['LOCALAPPDATA']))
        previous = request.get('from_product')
        if previous:
            old = root / 'bundles' / previous
            _, _, shell = installed(helper, old, manifest(old))
            if any(Path(p.get('ExecutablePath') or '') == shell for p in activity.get('processes', [])):
                raise ValueError('Quit the old Desktop through its native UI before upgrade')
        elif 'marker' in facts or facts['programs']['exists'] or facts['stable_cli']['exists']:
            raise ValueError('Fresh Desktop installation must not adopt an existing product')
        with (run / 'installer.stdout').open('xb') as out, (run / 'installer.stderr').open('xb') as err:
            done = subprocess.run([str(product / 'installer.exe'), '/S'], stdin=subprocess.DEVNULL,
                                  stdout=out, stderr=err, timeout=150, check=True)
        _, after, _ = installed(helper, product, pm)
        return dict(native_entry='NSIS /S', exit_code=done.returncode, installation=after,
                    upgrade_from=previous, installer_sha256=pm['files']['installer.exe']['sha256'])
    roots, facts, shell = installed(helper, product, pm)
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
                or args[0] not in ('system', 'service', 'agents', 'plans', 'compute', 'operations', 'console')):
            raise ValueError('Only bounded public product CLI families are accepted')
        environment = os.environ.copy()
        environment.update(request['config']['environment'])
        environment['PATH'] = ';'.join(request['config']['path']) + ';' + environment['PATH']
        done = subprocess.run([str(roots['stable_cli']), *args], capture_output=True, timeout=150, env=environment)
        (run / 'cli.stdout').write_bytes(done.stdout)
        (run / 'cli.stderr').write_bytes(done.stderr)
        return dict(native_entry='installed public CLI', exit_code=done.returncode)
    candidates = [p for p in activity.get('processes', []) if Path(p.get('ExecutablePath') or '') == shell]
    if len(candidates) != 1:
        raise ValueError('Exactly one measured installed Desktop process is required')
    pid = candidates[0]['ProcessId']
    element = None
    if action in ('invoke', 'text'):
        parent = root / 'runs' / request['observation_run']
        old_request = json.loads((parent / 'request.json').read_text(encoding='utf-8'))
        old_result = json.loads((parent / 'result.json').read_text(encoding='utf-8'))
        observed = json.loads((parent / 'ui.json').read_text(encoding='utf-8'))
        if (not old_result.get('passed') or old_request['product'] != request['product']
                or old_request['config']['session_id'] != request['config']['session_id']
                or observed['pid'] != pid or time.time() - (parent / 'ui.json').stat().st_mtime > 600):
            raise ValueError('A recent observation of this exact Desktop process is required')
        matches = [e for e in observed['nodes'] if e['runtime_id'] == request['element']]
        if len(matches) != 1:
            raise ValueError('Choose one observed native UI element')
        element = matches[0]
    return dict(native_entry='Windows UI Automation', observation=ui_action(run, request, shell, pid, element),
                shell_sha256=helper.digest(shell))
