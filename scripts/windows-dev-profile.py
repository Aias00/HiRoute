#!/usr/bin/env python3
"""Inspect and reversibly archive orphaned HiRoute data for native validation."""
import hashlib
import json
import os
from pathlib import Path
import stat


def digest(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def extended_windows_path(value):
    if value.startswith('\\\\?\\'):
        return value
    if value.startswith('\\\\'):
        return '\\\\?\\UNC\\' + value[2:]
    return '\\\\?\\' + value


def plain(path):
    path = Path(path)
    for entry in (path, *path.parents):
        try:
            info = entry.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 1024:
            raise ValueError('Reparse paths cannot be inspected or archived')
    return path


def locations(native_home, local_app_data):
    return {'data': native_home / '.local/share/hiroute',
            'state': local_app_data / 'HiRoute/cli',
            'shell_state': local_app_data / 'HiRoute/shell',
            'programs': local_app_data / 'Programs/HiRoute',
            'stable_cli': native_home / '.local/bin/hiroute.exe'}


def inspect_installation(native_home, local_app_data):
    """Read this account's installation facts without starting the product."""
    paths, facts = locations(native_home, local_app_data), {}
    for name, path in paths.items():
        plain(path)
        facts[name] = dict(path=str(path), exists=path.exists())
        if path.is_dir():
            facts[name]['entries'] = sorted(p.name for p in path.iterdir())[:50]
        elif path.is_file():
            facts[name].update(sha256=digest(path), bytes=path.stat().st_size)
    marker = plain(paths['data'] / 'standalone.json')
    if marker.is_file():
        if marker.stat().st_size > 64 * 1024:
            raise ValueError('Installation marker exceeds its format bound')
        record = json.loads(marker.read_text(encoding='utf-8'))
        facts['marker'] = dict(path=str(marker), sha256=digest(marker), record=record)
        for name, path in [('installed_cli', Path(record['install_root']) / 'hiroute.exe'),
                           ('definition', Path(record['service_definition']))]:
            plain(path)
            facts[name] = dict(path=str(path), exists=path.exists())
            if path.is_file():
                facts[name].update(sha256=digest(path), bytes=path.stat().st_size)
                if name == 'definition' and path.stat().st_size <= 64 * 1024:
                    facts[name]['text'] = path.read_text(encoding='utf-8', errors='replace')
    for name in ('shell-package.json', 'shell-install.json', 'uninstall.exe'):
        path = plain(paths['programs'] / name)
        if path.is_file():
            facts[name] = dict(path=str(path), sha256=digest(path), bytes=path.stat().st_size)
            if name.endswith('.json') and path.stat().st_size <= 64 * 1024:
                facts[name]['record'] = json.loads(path.read_text(encoding='utf-8'))
    for name in ('install-pending.json', 'package-cleanup.json', 'update-outcome.json'):
        path = plain(local_app_data / 'HiRoute/shell' / name)
        if path.is_file():
            facts[name] = dict(sha256=digest(path), bytes=path.stat().st_size,
                               modified_unix=path.stat().st_mtime)
            if path.stat().st_size <= 64 * 1024:
                facts[name]['record'] = json.loads(path.read_text(encoding='utf-8'))
    journal = plain(paths['data'] / 'service/install-journal.json')
    if journal.is_file():
        facts['install-journal.json'] = dict(sha256=digest(journal), bytes=journal.stat().st_size)
        if journal.stat().st_size <= 64 * 1024:
            facts['install-journal.json']['record'] = json.loads(journal.read_text(encoding='utf-8'))
    diagnostics = plain(paths['state'] / 'diagnostics')
    tails = {}
    if diagnostics.is_dir():
        for path in diagnostics.rglob('*.jsonl'):
            plain(path)
            if len(tails) == 16:
                break
            if path.is_file():
                size = path.stat().st_size
                with path.open('rb') as stream:
                    stream.seek(max(0, size - 64 * 1024))
                    lines = stream.read(64 * 1024).decode('utf-8', errors='replace').splitlines()
                records = []
                for line in lines:
                    try:
                        records.append(json.loads(line))
                    except ValueError:
                        pass  # A concurrently written or clipped first/last record is incomplete.
                tails[str(path.relative_to(diagnostics))] = dict(bytes=size, records=records)
    facts['diagnostics_tail'] = tails
    # Pre-replacement refusal may precede the transaction journal. Inspect only
    # fixed, bounded preparation metadata; never walk sealed history or infer a
    # restorable source from these staging files.
    retained = plain(paths['shell_state'] / 'recovery-packages')
    packages = []
    if retained.is_dir():
        from itertools import islice
        entries = list(islice(retained.iterdir(), 33))
        for entry in sorted(entries[:32]):
            plain(entry)
            if len(entry.name) != 64 or any(c not in '0123456789abcdef' for c in entry.name) or not entry.is_dir():
                continue
            files = {}
            for name in ('desktop/shell-package.json', 'program/package-manifest.json',
                         'installation/standalone.json', 'installation/windows-background.json',
                         'installation/task.xml', 'installation/uninstall.reg', 'restore-locations.json'):
                path = plain(entry / name)
                if path.is_file():
                    size = path.stat().st_size
                    files[name] = dict(bytes=size)
                    if size <= 4 * 1024 * 1024:
                        files[name]['sha256'] = digest(path)
            packages.append(dict(name=entry.name, files=files))
        facts['retained_preparation_metadata'] = dict(packages=packages, truncated=len(entries) > 32,
                                                     assessment='staging_only_not_recovery_admission')
    expected = facts.get('shell-package.json', {}).get('record', {}).get('files', {})
    mismatches = []
    for name, wanted in expected.items():
        if (len(name) >= 512 or '\\' in name or any(p in ('', '.', '..') for p in name.split('/'))):
            raise ValueError('Unsafe shell inventory path')
        path = plain(paths['programs'] / name)
        if path.exists() and digest(path) != wanted:
            mismatches.append(name)
    if expected:
        facts['shell_inventory_mismatches'] = mismatches
    backups = plain(local_app_data / 'HiRoute/shell/install-backups')
    if backups.is_dir():
        facts['shell_backup_entries'] = sorted(str(p.relative_to(backups)) for p in backups.rglob('*') if p.is_file())[:100]
        facts['shell_backup_mismatches'] = [name for name, wanted in expected.items()
                                           if (path := plain(backups / name)).is_file() and digest(path) != wanted]
    return facts


def inventory(root, exclude_startup_lock=False):
    """Reject links/special entries before walking or hashing any child."""
    if os.name == 'nt':
        root = Path(extended_windows_path(os.path.abspath(root)))
    def fail(error):
        raise error
    rows = {}
    for directory, children, files in os.walk(plain(root), followlinks=False, onerror=fail):
        for path in (Path(directory), *(Path(directory) / n for n in children + files)):
            plain(path)
            info = path.lstat()
            key = str(path.relative_to(root))
            if exclude_startup_lock and key == 'startup.lock':
                if not stat.S_ISREG(info.st_mode) or info.st_size != 0:
                    raise ValueError('Only the empty non-data startup lock may be excluded')
                continue
            if stat.S_ISDIR(info.st_mode):
                rows[key] = {'kind': 'directory'}
            elif stat.S_ISREG(info.st_mode):
                rows[key] = dict(kind='file', bytes=info.st_size, sha256=digest(path))
            else:
                raise ValueError('Unknown filesystem entry; retain the original data')
    return rows


def candidate_uninstaller(native_home, local_app_data, inspected, product, shell_sha256, activity):
    """Admit only the measured candidate's normal, data-preserving NSIS entry."""
    if activity.get('administrator_groups') is not False:
        raise ValueError('Candidate cleanup requires a standard account')
    current = inspect_installation(native_home, local_app_data)
    if current != inspected:
        raise ValueError('Installation changed since inspection')
    marker = current.get('marker', {}).get('record', {})
    install = Path(marker.get('install_root', ''))
    roots = locations(native_home, local_app_data)
    if (marker.get('schema_version') != 'hiroute.standalone-install/v1'
            or marker.get('target') != product['target']
            or install.parent != native_home / '.local/lib/hiroute'
            or not install.name.endswith('-' + product['source_revision'][:12])):
        raise ValueError('Installed product differs from the selected candidate')
    for name in ('hiroute.exe', 'hirouted.exe'):
        if digest(plain(install / name)) != product['files'][name]['sha256']:
            raise ValueError('Installed binary differs from the selected candidate')
    shell = plain(roots['programs'] / 'hiroute-shell.exe')
    uninstaller = plain(roots['programs'] / 'uninstall.exe')
    expected = current.get('shell-package.json', {}).get('record', {}).get('files', {})
    if (digest(shell) != shell_sha256 or expected.get('hiroute-shell.exe') != shell_sha256
            or digest(uninstaller) != expected.get('uninstall.exe')):
        raise ValueError('Uninstaller or shell differs from its measured inventory')
    definition = json.loads(current['definition']['text'])
    if definition.get('manager') != 'task':
        raise ValueError('Only the owned Task candidate can be removed')
    if any(Path(p.get('ExecutablePath') or '') != install / 'hirouted.exe'
           for p in activity.get('processes', [])):
        raise ValueError('Another product or installer is active; preserve it')
    if any(t.get('TaskName') != definition['name'] for t in activity.get('tasks', [])):
        raise ValueError('Another validation or product Task is active; preserve it')
    return uninstaller, install


def require_fresh_nsis_packages(native_home):
    packages = plain(native_home / '.local/lib/hiroute')
    if packages.exists() and (not packages.is_dir() or next(packages.iterdir(), None) is not None):
        raise ValueError('Fresh NSIS acceptance requires no retained package roots; use the Rust cleanup entry first')


def archive_orphaned_data(native_home, local_app_data, destination, inspected, activity):
    """Preserve every byte; installed products and live/managed data are refused."""
    if activity.get('administrator_groups') is not False:
        raise ValueError('A standard account without Administrators membership is required')
    if activity.get('processes') or activity.get('tasks'):
        raise ValueError('HiRoute activity remains; use the owning product stop/uninstall entry')
    current = inspect_installation(native_home, local_app_data)
    if current != inspected:
        raise ValueError('Installation facts changed since inspection; inspect again')
    if 'marker' in current or current['programs']['exists'] or current['stable_cli']['exists']:
        raise ValueError('An installed product remains; use its normal uninstall entry first')
    paths = locations(native_home, local_app_data)
    # Moving a managed artifact would invalidate its native Agent reference.
    archived = ('data', 'state', 'shell_state')
    roots = [str(paths[n]).replace('/', '\\').casefold() for n in archived]
    for name in ('.codex/config.toml', '.claude/settings.json',
                 '.agents/skills/hiroute-collaboration/SKILL.md'):
        path = plain(native_home / name)
        if path.is_file():
            if path.stat().st_size > 4 * 1024 * 1024:
                raise ValueError('Native configuration needs manual inspection')
            text = path.read_text(encoding='utf-8').replace('\\\\', '\\').replace('/', '\\').casefold()
            if any(root in text for root in roots):
                raise ValueError('Native Agent configuration still references the old data; restore it first')
    plans = []
    for name in archived:
        source, target = paths[name], plain(destination / ('previous-' + name))
        if not source.exists():
            continue
        if not source.is_dir():
            raise ValueError('Unexpected installation data entry; preserve it')
        if target.exists() or source.stat().st_dev != destination.stat().st_dev:
            raise ValueError('Backup needs an unused location on the same volume')
        plans.append((source, target, inventory(source)))
    record = dict(operation='archive_orphaned_installation_data', native_configuration='unchanged',
                  paths=[dict(source=str(s), backup=str(t), inventory=rows) for s, t, rows in plans])
    manifest = destination / 'profile-backup.json'
    with manifest.open('x', encoding='utf-8') as stream:
        stream.write(json.dumps(record, indent=2))
        stream.flush()
        os.fsync(stream.fileno())
    moved = []
    try:
        for source, target, expected in plans:
            if inventory(source) != expected:
                raise ValueError('Data changed before archival; preserve it')
            source.rename(target)
            moved.append((source, target))
            if inventory(target) != expected:
                raise ValueError('Archived data differs; restore the preserved directory')
    except Exception:
        for source, target in reversed(moved):
            if not source.exists():
                target.rename(source)
        (destination / 'profile-backup-outcome.json').write_text(json.dumps(dict(
            outcome='restored_after_error', manifest_sha256=digest(manifest))), encoding='utf-8')
        raise
    (destination / 'profile-backup-outcome.json').write_text(json.dumps(dict(
        outcome='archived_and_verified', manifest_sha256=digest(manifest))), encoding='utf-8')
    return dict(archived=[str(t) for _, t in moved], backup_manifest='profile-backup.json',
                native_configuration='unchanged', assessment='preparation_only')


def bounded_json(path, maximum=1024 * 1024):
    path = plain(path)
    if not path.is_file() or path.stat().st_size > maximum:
        raise ValueError('Missing or oversized recovery metadata')
    with path.open('rb') as stream:
        content = stream.read(maximum + 1)
    if len(content) > maximum:
        raise ValueError('Recovery metadata changed while reading')
    return json.loads(content)


def native_directory_identity(path):
    """The same volume/index pair as the product's retained-directory seal."""
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    class FileTime(ctypes.Structure):
        _fields_ = [('low', wintypes.DWORD), ('high', wintypes.DWORD)]
    class Information(ctypes.Structure):
        _fields_ = [('attributes', wintypes.DWORD), ('created', FileTime),
                    ('accessed', FileTime), ('written', FileTime),
                    ('volume', wintypes.DWORD), ('size_high', wintypes.DWORD),
                    ('size_low', wintypes.DWORD), ('links', wintypes.DWORD),
                    ('index_high', wintypes.DWORD), ('index_low', wintypes.DWORD)]
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
        ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.GetFileInformationByHandle.argtypes = [wintypes.HANDLE, ctypes.POINTER(Information)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    path = plain(path)
    handle = kernel.CreateFileW(str(path), 0x80, 7, None, 3, 0x02200000, None)
    if handle == wintypes.HANDLE(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        information = Information()
        if not kernel.GetFileInformationByHandle(handle, ctypes.byref(information)):
            raise ctypes.WinError(ctypes.get_last_error())
        if information.attributes & 1024 or not information.attributes & 16:
            raise ValueError('The sealed source is not an ordinary directory')
        return [information.volume, (information.index_high << 32) | information.index_low]
    finally:
        kernel.CloseHandle(handle)


def original_recovery_source(native_home, local_app_data, baseline):
    """Read the live locator's exact batch; never select a source by directory presence."""
    paths = locations(native_home, local_app_data)
    storage = plain(paths['state'] / 'storage')
    if os.name == 'nt':
        # Real retained native Agent material can exceed MAX_PATH under the
        # batch/source prefix. Match the existing offline inventory helper.
        storage = plain(extended_windows_path(str(storage)))
    locator_path = storage / 'observation-source-backup.json'
    locator = bounded_json(locator_path, 16 * 1024)
    import re
    def portable(value):
        return isinstance(value, str) and '..' not in value and re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', value)
    name, batch_id = locator.get('backup_name'), locator.get('set_id')
    if locator.get('schema') != 1 or not portable(name) or not portable(batch_id):
        raise ValueError('Unknown or foreign original-batch locator')
    parent = plain(storage.parent / (storage.name + '.upgrade-backups'))
    batch = plain(parent / name)
    row = bounded_json(batch / 'backup-set.manifest.json', 64 * 1024)
    if row.get('set_id') != batch_id:
        batch = plain(parent / ('migration-set.' + batch_id))
        row = bounded_json(batch / 'backup-set.manifest.json', 64 * 1024)
    if row.get('schema') != 'hiroute.sqlite-backup-set/v2' or row.get('set_id') != batch_id:
        raise ValueError('Original batch identity changed')
    source = plain(batch / 'source')
    record = bounded_json(source / 'source-backup.json')
    if record.get('schema') != 'hiroute.upgrade-source-backup/v2' or record.get('complete') is not True:
        raise ValueError('The original recovery source is not a complete sealed group')
    if record.get('source_info', {}).get('batch_id') != batch_id:
        raise ValueError('The original recovery source belongs to another batch')
    recorded_storage = record.get('source_storage')
    if (not isinstance(recorded_storage, str) or not Path(recorded_storage).is_absolute()
            or native_directory_identity(plain(recorded_storage)) != native_directory_identity(storage)):
        raise ValueError('The original recovery source belongs to another native storage directory')
    files = record.get('files')
    if not isinstance(files, dict) or not 1 <= len(files) <= 10000:
        raise ValueError('Original material inventory is unavailable')
    required = {'storage/live/' + name + '.db' for name in ('control', 'runtime', 'secrets')}
    required.update(('storage/master-key', 'storage/observation-hmac-key'))
    if not required <= files.keys():
        raise ValueError('The complete source lacks essential databases or keys')
    for name, expected in files.items():
        if (not isinstance(name, str) or '\\' in name or ':' in name
                or any(part in ('', '.', '..') for part in name.split('/'))
                or not isinstance(expected, str) or not re.fullmatch(r'sha256:[a-f0-9]{64}', expected)):
            raise ValueError('Invalid original material inventory')
        path = plain(source / name)
        if not path.is_file() or digest(path) != expected[7:]:
            raise ValueError('Original recovery material changed or is unavailable: ' + name)
    candidates = [source / 'recovery-package', source / 'recovery-package/program']
    matching = []
    for candidate in candidates:
        manifest = candidate / 'package-manifest.json'
        if not manifest.exists():
            continue
        package = bounded_json(manifest)
        if package.get('revision') != baseline['source_revision']:
            continue
        for binary in ('hiroute.exe', 'hirouted.exe'):
            if digest(plain(candidate / binary)) != baseline['files'][binary]['sha256']:
                raise ValueError('Original program does not match the independently staged baseline')
        matching.append(candidate)
    if len(matching) != 1:
        raise ValueError('The exact native baseline program cannot be identified')
    if native_directory_identity(source / 'storage/observation') != record.get('sealed_observation', {}).get('identity'):
        raise ValueError('Original sealed history identity changed')
    return dict(batch=batch, source=source, record=record, batch_id=batch_id,
                locator_sha256=digest(locator_path), program=matching[0])


def native_account_sid(account):
    """Resolve Scheduler's account-name spelling to its SID; never authorize by a display name."""
    import ctypes
    from ctypes import wintypes
    if os.name != 'nt' or not isinstance(account,str) or not account or len(account)>1024:
        raise ValueError('Native Windows account resolution is required')
    advapi=ctypes.WinDLL('advapi32',use_last_error=True)
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    advapi.LookupAccountNameW.argtypes=[wintypes.LPCWSTR,wintypes.LPCWSTR,ctypes.c_void_p,
        ctypes.POINTER(wintypes.DWORD),wintypes.LPWSTR,ctypes.POINTER(wintypes.DWORD),ctypes.POINTER(wintypes.DWORD)]
    advapi.ConvertSidToStringSidW.argtypes=[ctypes.c_void_p,ctypes.POINTER(wintypes.LPWSTR)]
    kernel.LocalFree.argtypes=[ctypes.c_void_p]
    size,domain_size,kind=wintypes.DWORD(),wintypes.DWORD(),wintypes.DWORD()
    advapi.LookupAccountNameW(None,account,None,ctypes.byref(size),None,ctypes.byref(domain_size),ctypes.byref(kind))
    if not 1<=size.value<=256 or not 1<=domain_size.value<=1024:
        raise ValueError('The native account could not be resolved within its bounds')
    sid=ctypes.create_string_buffer(size.value);domain=ctypes.create_unicode_buffer(domain_size.value)
    if not advapi.LookupAccountNameW(None,account,sid,ctypes.byref(size),domain,ctypes.byref(domain_size),ctypes.byref(kind)):
        raise ctypes.WinError(ctypes.get_last_error())
    result=wintypes.LPWSTR()
    if not advapi.ConvertSidToStringSidW(sid,ctypes.byref(result)):
        raise ctypes.WinError(ctypes.get_last_error())
    try:return result.value
    finally:kernel.LocalFree(result)


def original_desktop_locations(source, baseline, paths):
    """Admit only the installer-owned old destinations and actual limited-user Task."""
    import re
    import xml.etree.ElementTree as ET
    root = source['source'] / 'recovery-package'
    path = root / 'restore-locations.json'
    if not path.exists():
        return None
    value = bounded_json(path, 16 * 1024)
    required = {'schema','program','desktop','task_name','task_autostart','shell_login','files'}
    if (set(value) != required or value['schema'] != 'hiroute.windows-desktop-restore/v1'
            or type(value['task_autostart']) is not bool or type(value['shell_login']) is not bool):
        raise ValueError('Unknown original Desktop location record')
    package = bounded_json(source['program'] / 'package-manifest.json')
    version = package.get('version')
    home = paths['data'].parents[2]
    if not isinstance(version, str) or not re.fullmatch(r'[A-Za-z0-9_.+-]{1,128}',version):
        raise ValueError('Invalid original native package version')
    program = home / '.local/lib/hiroute' / (version + '-' + baseline['source_revision'][:12])
    if Path(value['program']) != program or Path(value['desktop'] or '') != paths['programs']:
        raise ValueError('Original programs belong to another installer namespace')
    if digest(plain(root/'desktop/hiroute-shell.exe')) != baseline['shell_sha256']:
        raise ValueError('The retained original shell differs from the independently staged NSIS')
    expected = {home/'.local/bin'/name: source['program']/name for name in ('hiroute.exe','hirouted.exe')}
    for prefix in ('.agents','.claude'):
        name = 'skills/hiroute-management/SKILL.md'
        expected[home/prefix/name] = source['program']/name
    for name in package.get('files', {}):
        if name.startswith(('docs/','licenses/')):
            expected[paths['data']/name] = source['program']/name
    expected[paths['data']/'package-manifest.json'] = source['program']/'package-manifest.json'
    expected[paths['data']/'standalone.json'] = root/'installation/standalone.json'
    expected[paths['data']/'service/windows-background.json'] = root/'installation/windows-background.json'
    files = value.get('files')
    if (not isinstance(files, dict) or len(files) != len(expected)
            or not {'installation/standalone.json','installation/windows-background.json'} <= files.keys()
            or {Path(target) for target in files.values()} != set(expected)):
        raise ValueError('The original native file map is incomplete or contains foreign destinations')
    for name, target in files.items():
        if (not re.fullmatch(r'installation/(standalone\.json|windows-background\.json|entry-[0-9]+\.bin)',name)
                or digest(plain(root/name)) != digest(plain(expected[Path(target)]))):
            raise ValueError('The original native entry is not paired with its package')
    marker = bounded_json(root/'installation/standalone.json',64*1024)
    if (marker.get('schema_version') != 'hiroute.standalone-install/v1'
            or Path(marker.get('install_root','')) != program or marker.get('target') != baseline['target']
            or Path(marker.get('service_definition','')) != paths['data']/'service/windows-background.json'):
        raise ValueError('The original installation marker is not paired with its programs and native account')
    definition = bounded_json(root/'installation/windows-background.json',64*1024)
    task = value['task_name']
    if (not re.fullmatch(r'HiRoute-[a-f0-9]{64}',task) or definition.get('name') != task
            or not isinstance(definition.get('owner_sid'),str)
            or task != 'HiRoute-' + hashlib.sha256(definition['owner_sid'].encode()).hexdigest()
            or definition.get('manager') != 'task' or Path(definition.get('daemon','')) != program/'hirouted.exe'
            or definition.get('daemon_sha256') != baseline['files']['hirouted.exe']['sha256']):
        raise ValueError('The original Task does not match its original native program')
    xml_path = plain(root/'installation/task.xml')
    if not xml_path.is_file() or xml_path.stat().st_size > 512*1024:
        raise ValueError('Original native Task XML is unavailable')
    xml = ET.fromstring(xml_path.read_bytes())
    ns = {'t':'http://schemas.microsoft.com/windows/2004/02/mit/task'}
    text = lambda name: xml.findtext(name, namespaces=ns)
    def owner(account):
        if account==definition['owner_sid']:return True
        try:return native_account_sid(account)==definition['owner_sid']
        except (ValueError,OSError):return False
    actions, triggers = xml.find('t:Actions',ns), xml.find('t:Triggers',ns)
    if (xml.tag != '{'+ns['t']+'}Task' or actions is None or len(actions) != 1 or triggers is None or len(triggers) != 1
            or not owner(text('t:Principals/t:Principal/t:UserId'))
            or text('t:Principals/t:Principal/t:RunLevel') != 'LeastPrivilege'
            or text('t:Principals/t:Principal/t:LogonType') != 'InteractiveToken'
            or Path(text('t:Actions/t:Exec/t:Command') or '') != program/'hirouted.exe'
            or Path(text('t:Actions/t:Exec/t:WorkingDirectory') or '') != program
            or text('t:Actions/t:Exec/t:Arguments') != '--role all --standalone'
            or not owner(text('t:Triggers/t:LogonTrigger/t:UserId'))
            or {None:True,'true':True,'1':True,'false':False,'0':False}.get(text('t:Triggers/t:LogonTrigger/t:Enabled')) != value['task_autostart']):
        raise ValueError('Original native Task XML has a foreign account, action, privilege or login policy')
    return value
