#!/usr/bin/env python3
"""Windows development-machine transport; personal locations stay in HOME."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import re
import subprocess
import sys
import tempfile
import uuid
import zipfile

REPO = Path(__file__).resolve().parent.parent
CONFIG = Path.home() / '.config/hiroute/windows-validation.json'
ENVIRONMENT = {
    'HIROUTE_WINDOWS_SOURCE_ROOT', 'HIROUTE_WORKER_NODE', 'HIROUTE_NATIVE_CODEX',
    'HIROUTE_WORKER_CODEX_BINARY', 'HIROUTE_WORKER_CLAUDE_BINARY',
    'HIROUTE_WORKER_QODER_BINARY', 'HIROUTE_WORKER_CODEX_ACP_ADAPTER',
    'HIROUTE_WORKER_CLAUDE_ACP_ADAPTER', 'HIROUTE_WORKER_PI_BINARY',
    'HIROUTE_WORKER_DSH_BINARY',
}


def digest(path):
    with Path(path).open('rb') as stream:
        value = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
        return value.hexdigest()


def windows_path(value):
    if not isinstance(value, str) or not PureWindowsPath(value).is_absolute() or any(
            c in value for c in '\x00\r\n') or '..' in PureWindowsPath(value).parts:
        raise ValueError('Expected an absolute Windows path without parent traversal')
    return value


def configuration(path):
    path = path.expanduser().resolve(strict=True)
    path.relative_to(Path.home().resolve())
    if path.is_relative_to(REPO):
        raise ValueError('Machine configuration belongs in HOME, outside the checkout')
    if os.name != 'nt' and path.stat().st_mode & 0o077:
        raise ValueError('Machine configuration requires mode 0600')
    value = json.loads(path.read_text())
    required = {'version', 'host', 'identity_file', 'python', 'root', 'user',
                'session_id', 'environment', 'path'}
    if set(value) != required or value['version'] != 1:
        raise ValueError('Expected windows-validation.json version 1 fields')
    if not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.@:-]*', value['host']):
        raise ValueError('Expected an SSH alias or user@host')
    identity = Path(value['identity_file']).expanduser().resolve(strict=True)
    identity.relative_to(Path.home().resolve())
    value['identity_file'] = str(identity)
    for key in ('python', 'root'):
        windows_path(value[key])
    if not re.fullmatch(r'[A-Za-z0-9_.\\@-]+', value['user']):
        raise ValueError('Expected an explicit Windows account')
    if type(value['session_id']) is not int or value['session_id'] < 1:
        raise ValueError('An existing interactive session ID is required')
    if not isinstance(value['environment'], dict) or set(value['environment']) - ENVIRONMENT:
        raise ValueError('Only declared client paths are accepted; credentials are not configuration')
    if not isinstance(value['path'], list):
        raise ValueError('path must contain Windows discovery directories')
    for entry in [*value['environment'].values(), *value['path']]:
        windows_path(entry)
    return value


def powershell(script):
    return 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(
        script.encode('utf-16le')).decode()


def ps_literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def python_command(binary, code):
    # Windows PowerShell native argv processing can remove Python's embedded
    # double quotes. Transfer the small bootstrap as data, not source in argv.
    encoded = base64.b64encode(code.encode('utf-8')).decode()
    return ('& ' + ps_literal(binary) + ' -c "import base64;exec(base64.b64decode(\'' +
            encoded + '\'))"; exit $LASTEXITCODE')


class Remote:
    def __init__(self, config):
        self.config = config
        self.options = ['-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                        '-o', 'IdentitiesOnly=yes', '-o', 'ConnectTimeout=15',
                        '-i', config['identity_file']]

    def execute(self, script, timeout=65):
        result = subprocess.run(['ssh', *self.options, self.config['host'], powershell(script)],
                                capture_output=True, text=True, timeout=timeout)
        if result.returncode:
            raise RuntimeError('Windows transport failed; no automatic replay was attempted: ' +
                               result.stderr[-2000:])
        return result.stdout

    def upload(self, source, destination):
        subprocess.run(['scp', '-q', *self.options, str(source),
                        self.config['host'] + ':' + destination], check=True, timeout=180)

    def bootstrap(self):
        # Uploading source avoids Windows command-line length limits. Only a small
        # bootstrap enters argv; request data is a separately hashed JSON file.
        root = self.config['root']
        code = "from pathlib import Path; import os; p=Path(" + repr(root) + "); " + (
            "assert all(not x.exists() or not (x.lstat().st_file_attributes & 1024) "
            "for x in [p,*p.parents]); p.mkdir(parents=True,exist_ok=True)")
        self.execute(python_command(self.config['python'], code))
        source = REPO / 'scripts/windows-dev-host.py'
        self.driver_hash = digest(source)
        self.driver = root + '/driver-' + self.driver_hash + '.py'
        self.upload(source, self.driver)

    def call(self, action, **arguments):
        paths = ['scripts/windows-dev.py', 'scripts/windows-dev-host.py', 'scripts/validation.py']
        if action == 'run':
            subprocess.run(['git', 'ls-files', '--error-unmatch', *paths], cwd=REPO,
                           stdout=subprocess.DEVNULL, check=True)
            if subprocess.check_output(['git', 'status', '--porcelain', '--', *paths], cwd=REPO):
                raise ValueError('Commit the runner before native product execution')
        request = dict(action=action, config=self.config, driver_sha256=self.driver_hash,
                       driver_revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
                       driver_path=self.driver, **arguments)
        # SSH identity and host never need to reach the Windows execution process.
        request['config'] = {k: v for k, v in self.config.items()
                             if k not in ('host', 'identity_file')}
        with tempfile.TemporaryDirectory(prefix='hiroute-windows-request-') as temporary:
            path = Path(temporary) / 'request.json'
            path.write_text(json.dumps(request), encoding='utf-8')
            checksum = digest(path)
            remote = self.config['root'] + '/request-' + checksum + '.json'
            self.upload(path, remote)
        code = ('from pathlib import Path;import hashlib,runpy,sys; p=Path(' + repr(self.driver) +
                ');assert hashlib.sha256(p.read_bytes()).hexdigest()==' + repr(self.driver_hash) +
                ';sys.argv=[str(p),' + repr(remote) + ',' + repr(checksum) + '];runpy.run_path(str(p),run_name="__main__")')
        output = self.execute(python_command(self.config['python'], code))
        return json.loads(output)


def bundle(result_path, log_path, output):
    result = json.loads(result_path.read_text())
    sha = result['sha']
    if result['process_exit'] != 0 or not re.fullmatch('[a-f0-9]{40}', sha):
        raise ValueError('A successful revision-bound managed build is required')
    checkout = Path(result['checkout']).resolve(strict=True)
    if subprocess.check_output(['git', '-C', str(checkout), 'rev-parse', 'HEAD'], text=True).strip() != sha:
        raise ValueError('Build checkout has moved; export before continuing it')
    images = {}
    for line in log_path.read_text().splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get('reason') != 'compiler-artifact' or not row.get('executable'):
            continue
        path = Path(row['executable']).resolve(strict=True)
        path.relative_to(checkout / 'target/x86_64-pc-windows-msvc')
        name = row['target']['name'] + '.exe'
        if name in images and images[name] != path:
            raise ValueError('Ambiguous binary target; export one explicit build')
        images[name] = path
    if not images:
        raise ValueError('Build log selected zero executable artifacts')
    manifest = dict(source_revision=sha, target='x86_64-pc-windows-msvc',
                    producer_platform=sys.platform, native_ci_assessment='not_assessed',
                    run_id=result.get('id'), command=result.get('command'),
                    cargo_lock_sha256=digest(checkout / 'Cargo.lock'), files={})
    with zipfile.ZipFile(output, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
        for name, path in images.items():
            if not re.fullmatch(r'[A-Za-z0-9_-]+\.exe', name):
                raise ValueError('Invalid executable name')
            manifest['files'][name] = dict(sha256=digest(path), bytes=path.stat().st_size)
            archive.write(path, name)
        archive.writestr('manifest.json', json.dumps(manifest))
    return dict(bundle_sha256=digest(output), source_revision=sha, files=list(images))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, default=CONFIG)
    commands = parser.add_subparsers(dest='action', required=True)
    commands.add_parser('doctor')
    stage = commands.add_parser('stage')
    stage.add_argument('archive', type=Path)
    run = commands.add_parser('run')
    run.add_argument('--harness', required=True)
    run.add_argument('--product', required=True)
    run.add_argument('--binary', default='windows_headless.exe')
    run.add_argument('--case', required=True)
    run.add_argument('--ignored', action='store_true')
    run.add_argument('--timeout', type=int, default=900, help='Whole test process watchdog, not CLI timeout')
    for action in ('status', 'wait', 'logs', 'finish'):
        command = commands.add_parser(action)
        command.add_argument('run_id')
        if action == 'wait':
            command.add_argument('--timeout', type=int, default=50, choices=range(1, 51))
        if action == 'logs':
            command.add_argument('--tail', type=int, default=60, choices=range(1, 201))
    collect = commands.add_parser('collect')
    collect.add_argument('run_id')
    collect.add_argument('--output', type=Path, required=True)
    export = commands.add_parser('bundle')
    export.add_argument('--result', type=Path, required=True)
    export.add_argument('--log', type=Path, required=True)
    export.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.action == 'bundle':
        value = bundle(args.result, args.log, args.output)
    else:
        remote = Remote(configuration(args.config))
        remote.bootstrap()
        values = vars(args).copy()
        for key in ('config', 'action'):
            values.pop(key)
        if args.action == 'stage':
            checksum = digest(args.archive)
            destination = remote.config['root'] + '/upload-' + checksum + '.zip'
            remote.upload(args.archive, destination)
            values = dict(archive=destination, sha256=checksum)
        elif args.action == 'run':
            values['run_id'] = uuid.uuid4().hex
        elif args.action == 'collect':
            values.pop('output')
        value = remote.call(args.action, **values)
        if args.action == 'collect':
            args.output.mkdir(parents=True, exist_ok=False, mode=0o700)
            for name in value['files']:
                if not re.fullmatch(r'[A-Za-z0-9_.-]+', name):
                    raise ValueError('Invalid evidence filename')
                destination = args.output / name
                subprocess.run(['scp', '-q', *remote.options, remote.config['host'] + ':' +
                                value['directory'] + '/' + name, str(destination)], check=True, timeout=180)
                destination.chmod(0o600)
    print(json.dumps(value, indent=2))
    if args.action in ('status', 'wait') and value.get('state') == 'failed':
        return 1
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
