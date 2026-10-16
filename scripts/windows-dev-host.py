#!/usr/bin/env python3
"""Hash-verified Windows executor for windows-dev.py; no machine-specific defaults."""
import base64
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import time
import zipfile


def digest(path):
    with Path(path).open('rb') as stream:
        value = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
        return value.hexdigest()


def plain(path):
    path = Path(path)
    for entry in [path, *path.parents]:
        if entry.exists() and (entry.is_symlink() or
                getattr(entry.lstat(), 'st_file_attributes', 0) & 1024):
            raise ValueError('Reparse paths are not development-runner storage')
    return path


def identifier(value, pattern=r'[a-f0-9]{32}'):
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise ValueError('Invalid runner identifier')
    return value


def pe(path):
    with path.open('rb') as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b'MZ':
            raise ValueError('Expected a native executable')
        stream.seek(struct.unpack_from('<I', header, 60)[0])
        if stream.read(6) != b'PE\0\0\x64\x86':
            raise ValueError('Expected an x64 PE artifact')


def manifest(directory):
    plain(directory)
    value = json.loads((directory / 'manifest.json').read_text(encoding='utf-8'))
    identifier(value['source_revision'], r'[a-f0-9]{40}')
    if value['target'] != 'x86_64-pc-windows-msvc' or not value['files']:
        raise ValueError('Expected explicit Windows MSVC artifacts')
    for name, expected in value['files'].items():
        identifier(name, r'[A-Za-z0-9_-]+\.exe')
        file = plain(directory / name)
        if file.stat().st_nlink != 1 or file.stat().st_size != expected['bytes'] or digest(file) != expected['sha256']:
            raise ValueError('Artifact identity differs from its manifest')
        pe(file)
    return value


def stage(root, archive, checksum):
    archive = plain(archive)
    if digest(archive) != checksum:
        raise ValueError('Transferred archive hash differs')
    with zipfile.ZipFile(archive) as source:
        names = source.namelist()
        if len(names) != len(set(names)) or 'manifest.json' not in names:
            raise ValueError('Duplicate or absent manifest')
        for item in source.infolist():
            if not re.fullmatch(r'(manifest\.json|[A-Za-z0-9_-]+\.exe)', item.filename):
                raise ValueError('Only flat executable bundles are accepted')
            if (item.external_attr >> 16) & 0o170000 == 0o120000 or item.file_size > 512 * 1024 * 1024:
                raise ValueError('Unsupported archive entry')
        if sum(item.file_size for item in source.infolist()) > 2 * 1024**3:
            raise ValueError('Bundle exceeds development artifact bound')
        value = json.loads(source.read('manifest.json'))
        identifier(value['source_revision'], r'[a-f0-9]{40}')
        if set(names) != {'manifest.json', *value['files']}:
            raise ValueError('Unmanifested archive contents')
        name = value['source_revision'][:12] + '-' + checksum[:12]
        target = plain(root / 'bundles' / name)
        if not target.exists():
            target.mkdir(parents=True)
            try:
                source.extractall(target)
                manifest(target)
            except Exception:
                # Keep failed bytes for diagnosis, never reuse a partial stage.
                (target / 'invalid').touch()
                raise
        if (target / 'invalid').exists():
            raise ValueError('Previous stage failed; preserve it and use a corrected bundle')
        verified = manifest(target)
    return dict(bundle=name, source_revision=verified['source_revision'], files=list(verified['files']))


def ps(script):
    result = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-EncodedCommand',
                             base64.b64encode(script.encode('utf-16le')).decode()],
                            capture_output=True, text=True, errors='replace', timeout=30)
    if result.returncode:
        raise RuntimeError('Scheduled task operation failed: ' + result.stderr[-1800:])
    return result.stdout.strip()


def literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def task_name(run_id):
    return 'HiRoute-Dev-' + identifier(run_id)


def task_state(run_id):
    value = ps("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
               '$t=Get-ScheduledTask -TaskName ' + literal(task_name(run_id)) +
               ' -ErrorAction SilentlyContinue; if($t){$i=$t|Get-ScheduledTaskInfo;'
               '@{state=[string]$t.State;result=$i.LastTaskResult}|ConvertTo-Json -Compress}'
               'else{\'{"state":"absent"}\'}')
    return json.loads(value)


def directory(root, run_id):
    return plain(root / 'runs' / identifier(run_id))


def private_runs(root, user):
    path = plain(root / 'runs')
    # Raw native output stays under a private runner-owned parent. Never repair
    # permissions on an existing directory with an unrelated access policy.
    script = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
    script += '$path=' + literal(path) + ';$account=' + literal(user) + ';'
    script += r'''
$sid=[Security.Principal.SecurityIdentifier]
$current=[Security.Principal.WindowsIdentity]::GetCurrent().User
$target=([Security.Principal.NTAccount]::new($account)).Translate($sid)
$allowed=@($current.Value,$target.Value,'S-1-5-18','S-1-5-32-544')|Select-Object -Unique
if(-not (Test-Path -LiteralPath $path)) {
 New-Item -ItemType Directory -Path $path | Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new()
 $acl.SetOwner($current);$acl.SetAccessRuleProtection($true,$false)
 foreach($id in $allowed){
  $rule=[Security.AccessControl.FileSystemAccessRule]::new(
   [Security.Principal.SecurityIdentifier]::new($id),'FullControl','ContainerInherit,ObjectInherit','None','Allow')
  $acl.AddAccessRule($rule)
 }
 Set-Acl -LiteralPath $path -AclObject $acl
}
$acl=Get-Acl -LiteralPath $path
if(-not $acl.AreAccessRulesProtected){throw 'Runner evidence directory inherits foreign access'}
foreach($rule in $acl.GetAccessRules($true,$true,$sid)){
 if($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Value -notin $allowed){
  throw 'Runner evidence directory admits an unrelated identity'
 }
}
'''
    ps(script)


def status(root, run_id):
    run = directory(root, run_id)
    task = task_state(run_id)
    result = run / 'result.json'
    if result.exists():
        value = json.loads(result.read_text(encoding='utf-8'))
        return dict(run_id=run_id, state='passed' if value['passed'] else 'failed',
                    task=task, result=value)
    return dict(run_id=run_id, state='running' if task['state'] == 'Running' else 'failed',
                task=task, reason='No terminal result yet; task exit alone cannot prove a test')


def start(root, request):
    config = request['config']
    run_id = identifier(request['run_id'])
    if not 1 <= request['timeout'] <= 1800:
        raise ValueError('Whole-case watchdog must be between 1 and 1800 seconds')
    for field in ('harness', 'product'):
        identifier(request[field], r'[a-f0-9]{12}-[a-f0-9]{12}')
        manifest(root / 'bundles' / request[field])
    identifier(request['binary'], r'[A-Za-z0-9_-]+\.exe')
    identifier(request['case'], r'[A-Za-z0-9_:]+')
    private_runs(root, config['user'])
    lock = root / 'active.json'
    # A disconnected controller must not permit concurrent mutation of the same
    # native account. Explicit finish retires only a verified stopped task.
    with lock.open('x', encoding='utf-8') as stream:
        json.dump(dict(run_id=run_id), stream)
    run = directory(root, run_id)
    run.mkdir(parents=True)
    (run / 'request.json').write_text(json.dumps(request), encoding='utf-8')
    arguments = subprocess.list2cmdline([request['driver_path'], '--worker', str(run / 'request.json')])
    script = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
    script += '$name=' + literal(task_name(run_id)) + ';'
    script += "if(Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue){throw 'Task already exists'};"
    script += '$a=New-ScheduledTaskAction -Execute ' + literal(config['python']) + ' -Argument ' + literal(arguments) + ';'
    script += '$p=New-ScheduledTaskPrincipal -UserId ' + literal(config['user']) + ' -LogonType Interactive -RunLevel Limited;'
    script += '$s=New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds ' + str(request['timeout'] + 90) + ') -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries;'
    script += 'Register-ScheduledTask -TaskName $name -Action $a -Principal $p -Settings $s|Out-Null;Start-ScheduledTask -TaskName $name'
    ps(script)
    return dict(run_id=run_id, state='submitted', task=task_name(run_id))


def worker(request):
    config = request['config']
    root = plain(config['root'])
    run = directory(root, request['run_id'])
    result = dict(passed=False, case=request['case'], driver_sha256=request['driver_sha256'],
                  driver_revision=request['driver_revision'],
                  os=platform.platform(), machine=platform.machine(), source_evidence='development only')
    started = time.monotonic()
    child = None
    try:
        if digest(Path(__file__)) != request['driver_sha256']:
            raise ValueError('Worker driver changed')
        session = ctypes.c_ulong()
        if not ctypes.windll.kernel32.ProcessIdToSessionId(os.getpid(), ctypes.byref(session)):
            raise OSError('Cannot resolve actual Windows session')
        user = os.environ['USERNAME']
        if user.casefold() != config['user'].split('\\')[-1].casefold() or session.value != config['session_id']:
            raise ValueError('The configured account is not in the expected interactive session')
        result.update(session_id=session.value, administrator=bool(ctypes.windll.shell32.IsUserAnAdmin()))
        harness = root / 'bundles' / request['harness']
        product = root / 'bundles' / request['product']
        hm, pm = manifest(harness), manifest(product)
        result.update(harness_revision=hm['source_revision'], product_revision=pm['source_revision'],
                      harness_files=hm['files'], product_files=pm['files'])
        if request['binary'] not in hm['files'] or not {'hiroute.exe', 'hirouted.exe'} <= set(pm['files']):
            raise ValueError('The selected manifests do not include required binaries')
        # Never manufacture HOME or remove an existing installation to make a gate pass.
        home = Path.home()
        for path in (Path(os.environ['LOCALAPPDATA']) / 'HiRoute/cli',
                     home / '.local/share/hiroute/standalone.json', home / '.local/bin/hiroute.exe'):
            if path.exists():
                raise ValueError('Disposable installed-product gate found an existing installation')
        environment = os.environ.copy()
        environment.update(config['environment'])
        for path in config['environment'].values():
            if not Path(path).exists():
                raise ValueError('Configured native client prerequisite is absent')
        fixture = Path(environment['HIROUTE_WINDOWS_SOURCE_ROOT']) / 'tools/windows-agent-fixtures'
        dependencies = json.loads((fixture / 'package.json').read_text(encoding='utf-8'))['dependencies']
        for name, version in dependencies.items():
            actual = json.loads((fixture / 'node_modules' / name / 'package.json').read_text(encoding='utf-8'))
            if actual['version'] != version:
                raise ValueError('Installed native client differs from pinned fixture')
        result['package_lock_sha256'] = digest(fixture / 'package-lock.json')
        result['clients'] = {name: digest(Path(path)) for name, path in config['environment'].items()
                             if Path(path).is_file()}
        environment.update(HIROUTE_VALIDATION_PRODUCT_BIN_DIR=str(product),
                           HIROUTE_VALIDATION_EVIDENCE_DIR=str(run),
                           HIROUTE_WORKER_CLI_BINARY=str(product / 'hiroute.exe'),
                           HIROUTE_WORKER_DAEMON_BINARY=str(product / 'hirouted.exe'))
        environment['PATH'] = ';'.join(config['path']) + ';' + environment['PATH']
        binary = harness / request['binary']
        listed = subprocess.run([str(binary), '--list', '--format', 'terse'], env=environment,
                                capture_output=True, text=True, timeout=30, check=True)
        if listed.stdout.splitlines().count(request['case'] + ': test') != 1:
            raise ValueError('Exact required test is missing or ambiguous')
        args = [str(binary), request['case'], '--exact', '--nocapture', '--test-threads=1']
        if request['ignored']:
            args.append('--ignored')
        with (run / 'stdout.log').open('xb') as output, (run / 'stderr.log').open('xb') as error:
            child = subprocess.Popen(args, env=environment, stdout=output, stderr=error)
            while child.poll() is None:
                if time.monotonic() - started > request['timeout']:
                    subprocess.run(['taskkill.exe', '/PID', str(child.pid), '/T', '/F'],
                                   capture_output=True, timeout=30, check=True)
                    child.wait(timeout=30)
                    raise TimeoutError('Whole test process exceeded its watchdog')
                diagnostics = Path(os.environ['LOCALAPPDATA']) / 'HiRoute/cli/diagnostics'
                for path in diagnostics.rglob('*.jsonl'):
                    try:
                        if path.stat().st_size <= 8 * 1024 * 1024:
                            name = hashlib.sha256(str(path.relative_to(diagnostics)).encode()).hexdigest()[:16]
                            shutil.copyfile(plain(path), run / ('diagnostics-' + name + '.jsonl'))
                    except (PermissionError, FileNotFoundError):
                        pass
                time.sleep(0.5)
        stdout = (run / 'stdout.log').read_text(encoding='utf-8', errors='replace')
        result.update(exit_code=child.returncode, passed=child.returncode == 0 and
                      bool(re.search(r'test result: ok\. 1 passed; 0 failed; 0 ignored;', stdout)))
    except Exception as error:
        result.update(error_type=type(error).__name__, error=str(error))
    finally:
        if child is not None and child.poll() is None:
            subprocess.run(['taskkill.exe', '/PID', str(child.pid), '/T', '/F'], capture_output=True, timeout=30)
            child.wait(timeout=30)
        result['seconds'] = round(time.monotonic() - started, 3)
        temporary = run / 'result.tmp'
        temporary.write_text(json.dumps(result, indent=2), encoding='utf-8')
        temporary.replace(run / 'result.json')
    return 0 if result['passed'] else 1


def finish(root, run_id):
    run = directory(root, run_id)
    request = json.loads((run / 'request.json').read_text(encoding='utf-8'))
    config = request['config']
    expected = subprocess.list2cmdline([request['driver_path'], '--worker', str(run / 'request.json')])
    script = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
    script += '$t=Get-ScheduledTask -TaskName ' + literal(task_name(run_id)) + ';'
    script += "if($t.State -ne 'Ready' -or $t.Actions.Execute -ne " + literal(config['python'])
    script += ' -or $t.Actions.Arguments -ne ' + literal(expected) + "){throw 'Owned task is active or differs'};"
    script += '$t|Unregister-ScheduledTask -Confirm:$false'
    ps(script)
    lock = plain(root / 'active.json')
    if json.loads(lock.read_text())['run_id'] != run_id:
        raise ValueError('Active owner differs; lock retained')
    lock.unlink()
    return dict(run_id=run_id, task_removed=True, evidence_retained=True,
                native_configuration='not modified by finish')


def recover(root, run_id):
    """Preserve only identifiable files left by this stopped disposable business case."""
    run = directory(root, run_id)
    request = json.loads((run / 'request.json').read_text(encoding='utf-8'))
    result = json.loads((run / 'result.json').read_text(encoding='utf-8'))
    if (root / 'active.json').exists() or task_state(run_id)['state'] != 'absent':
        raise ValueError('Finish the stopped run before recovering its owned configuration')
    if result['passed'] or request['case'] not in (
            'actual_windows_complete_public_headless_business',
            'actual_windows_public_headless_business_without_claude'):
        raise ValueError('Recovery is limited to failed disposable business cases')
    if os.environ['USERNAME'].casefold() != request['config']['user'].casefold():
        raise ValueError('Recovery must use the same native account')
    home = Path.home()
    state = Path(os.environ['LOCALAPPDATA']) / 'HiRoute/cli'
    if any(p.exists() for p in (state, home / '.local/bin/hiroute.exe',
                               home / '.local/share/hiroute/standalone.json')):
        raise ValueError('An installed product remains; use its public restore operation')
    lower, upper = (run / 'request.json').stat().st_mtime, (run / 'result.json').stat().st_mtime
    candidates = [('codex-config.toml', home / '.codex/config.toml'),
                  ('collaboration-skill.md', home / '.agents/skills/hiroute-collaboration/SKILL.md'),
                  ('claude-settings.json', home / '.claude/settings.json')]
    admitted = []
    for name, path in candidates:
        if not path.exists():
            continue
        plain(path)
        info = path.stat()
        if info.st_nlink != 1 or not lower <= info.st_mtime <= upper or info.st_size > 1024 * 1024:
            raise ValueError('Native configuration identity or modification time is outside this run')
        body = path.read_bytes()
        if name == 'codex-config.toml':
            import tomllib
            value = tomllib.loads(body.decode('utf-8'))
            if set(value) != {'model', 'model_provider', 'model_catalog_json', 'model_providers'} or value['model_provider'] != 'hiroute':
                raise ValueError('Unrecognized Codex configuration; retain it')
            Path(value['model_catalog_json']).relative_to(state / 'storage/managed-artifacts/agent-connections')
        elif name == 'claude-settings.json':
            if json.loads(body) != {'windows_headless_preserved': True}:
                raise ValueError('Claude configuration is not the untouched disposable marker')
        elif (list(path.parent.iterdir()) != [path] or
              b'hiroute worker plans' not in body or b'hiroute worker exec' not in body):
            raise ValueError('Unrecognized collaboration skill; retain it')
        destination = run / ('recovered-' + name)
        if destination.exists():
            raise ValueError('Recovery evidence already exists')
        admitted.append((path, destination, digest(path)))
    for path, destination, checksum in admitted:
        if digest(path) != checksum:
            raise ValueError('Native file changed during recovery')
        path.rename(destination)
        if path.name == 'SKILL.md':
            path.parent.rmdir()
    return dict(run_id=run_id, preserved=[p.name for _, p, _ in admitted])


def execute(request):
    root = plain(request['config']['root'])
    action = request['action']
    if action == 'doctor':
        return dict(os=platform.platform(), machine=platform.machine(), python=sys.version.split()[0],
                    configured_session=request['config']['session_id'],
                    sessions=subprocess.run(['query.exe', 'user'], capture_output=True, text=True,
                                            errors='replace', timeout=15).stdout,
                    active=json.loads((root / 'active.json').read_text()) if (root / 'active.json').exists() else None)
    if action == 'stage':
        return stage(root, request['archive'], request['sha256'])
    if action == 'run':
        return start(root, request)
    run = directory(root, request['run_id'])
    if action in ('status', 'wait'):
        if action == 'wait':
            end = time.monotonic() + min(50, max(1, request['timeout']))
            while not (run / 'result.json').exists() and time.monotonic() < end:
                time.sleep(1)
        return status(root, request['run_id'])
    if action == 'logs':
        return {name: (run / name).read_text(encoding='utf-8', errors='replace').splitlines()[-request['tail']:]
                for name in ('stdout.log', 'stderr.log') if (run / name).exists()}
    if action == 'collect':
        if task_state(request['run_id'])['state'] == 'Running':
            raise ValueError('Wait for the runner to stop before collecting final evidence')
        return dict(directory=str(run), files=[p.name for p in run.iterdir() if p.is_file()])
    if action == 'recover':
        return recover(root, request['run_id'])
    if action == 'finish':
        return finish(root, request['run_id'])
    raise ValueError('Unknown Windows development action')


if __name__ == '__main__':
    if sys.platform != 'win32':
        raise SystemExit('Native Windows required')
    if sys.argv[1] == '--worker':
        sys.exit(worker(json.loads(Path(sys.argv[2]).read_text(encoding='utf-8'))))
    path = plain(sys.argv[1])
    if digest(path) != sys.argv[2]:
        raise SystemExit('Request hash differs')
    print(json.dumps(execute(json.loads(path.read_text(encoding='utf-8')))))
