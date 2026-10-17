#!/usr/bin/env python3
"""Development-runner admission regressions, not native product evidence."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


controller, host = load('windows-dev'), load('windows-dev-host')


class Runner(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()

    def archive(self, extra=None, mutate=None):
        image = bytearray(70)
        image[:2] = b'MZ'
        struct.pack_into('<I', image, 60, 64)
        image[64:] = b'PE\0\0\x64\x86'
        binary = self.root / 'sample.exe'
        binary.write_bytes(image)
        value = dict(source_revision='a' * 40, target='x86_64-pc-windows-msvc',
                     files={'sample.exe': dict(sha256=controller.digest(binary), bytes=len(image))})
        if mutate:
            mutate(value)
        archive = self.root / 'sample.zip'
        with zipfile.ZipFile(archive, 'w') as stream:
            stream.writestr('manifest.json', json.dumps(value))
            stream.write(binary, 'sample.exe')
            if extra:
                stream.writestr(*extra)
        return archive

    def test_stage_verifies_bytes_and_rechecks_existing_bundle(self):
        archive = self.archive()
        value = host.stage(self.root, archive, controller.digest(archive))
        self.assertEqual(value['source_revision'], 'a' * 40)
        self.assertEqual(host.stage(self.root, archive, controller.digest(archive)), value)
        (self.root / 'bundles' / value['bundle'] / 'sample.exe').write_bytes(b'changed')
        with self.assertRaises(ValueError):
            host.stage(self.root, archive, controller.digest(archive))

    def test_transfer_hash_is_required_before_extraction(self):
        archive = self.archive()
        with self.assertRaises(ValueError):
            host.stage(self.root, archive, '0' * 64)
        self.assertFalse((self.root / 'bundles').exists())

    def test_archive_traversal_and_unmanifested_entries_fail(self):
        for name in ('../escape', '/absolute', 'C:/absolute', 'dir\\file.exe', 'other.exe'):
            with self.subTest(name=name):
                archive = self.archive(extra=(name, b'bad'))
                with self.assertRaises(ValueError):
                    host.stage(self.root, archive, controller.digest(archive))
        self.assertFalse((self.root / 'bundles').exists())

    def test_failed_stage_is_not_reused(self):
        archive = self.archive(mutate=lambda v: v['files']['sample.exe'].update(sha256='0' * 64))
        with self.assertRaises(ValueError):
            host.stage(self.root, archive, controller.digest(archive))
        with self.assertRaisesRegex(ValueError, 'Previous stage failed'):
            host.stage(self.root, archive, controller.digest(archive))

    def test_wrong_architecture_is_rejected(self):
        archive = self.archive()
        binary = self.root / 'sample.exe'
        content = bytearray(binary.read_bytes())
        content[-2:] = b'\x64\xaa'
        binary.write_bytes(content)
        with self.assertRaises(ValueError):
            host.pe(binary)

    def test_nsis_stub_is_admitted_only_as_the_declared_installer(self):
        payload = bytearray(70)
        payload[:2] = b'MZ'
        struct.pack_into('<I', payload, 60, 64)
        payload[64:] = b'PE\0\0\x4c\x01'
        entry = dict(sha256=hashlib.sha256(payload).hexdigest(), bytes=len(payload))
        for declaration in (None, 'nsis'):
            def mutate(value):
                value['files']['installer.exe'] = entry
                if declaration:
                    value['installer_format'] = declaration
            archive = self.archive(extra=('installer.exe', payload), mutate=mutate)
            if declaration:
                staged = host.stage(self.root, archive, controller.digest(archive))
                self.assertIn('installer.exe', staged['files'])
                path = self.root / 'bundles' / staged['bundle'] / 'installer.exe'
                with self.assertRaises(ValueError):
                    host.pe(path)  # Ordinary product/harness admission stays x64.
            else:
                with self.assertRaisesRegex(ValueError, 'explicit NSIS'):
                    host.stage(self.root, archive, controller.digest(archive))

    def test_desktop_bundle_binds_installer_and_compiled_harness_to_the_candidate(self):
        sha = 'a' * 40
        binaries = self.root / 'target/x86_64-pc-windows-msvc/release'
        binaries.mkdir(parents=True)
        harness = binaries / 'windows_desktop.exe'
        harness.write_bytes(b'component-test-harness')
        payload = self.root / 'apps/shell/src-tauri/resources/standalone'
        payload.mkdir(parents=True)
        for name in ('hiroute.exe', 'hirouted.exe'):
            (payload / name).write_bytes(b'embedded-' + name.encode())
        (payload / 'package-manifest.json').write_text(json.dumps(dict(revision=sha,
            target='x86_64-pc-windows-msvc', files={
                'bin/' + name: dict(sha256=controller.digest(payload / name), size=(payload / name).stat().st_size)
                for name in ('hiroute.exe', 'hirouted.exe')})))
        assets = self.root / 'target/candidate/assets'
        assets.mkdir(parents=True)
        installer = assets / 'candidate.exe'
        installer.write_bytes(b'component-nsis-fixture')
        result = dict(revision=sha, target='x86_64-pc-windows-msvc', status='awaiting_runtime',
                      package_integrity='green', assets=str(assets),
                      desktop_result=dict(revision=sha, native_runtime='not_executed',
                                          installer_sha256=controller.digest(installer), installer_size=installer.stat().st_size,
                                          standalone_manifest_sha256=controller.digest(payload / 'package-manifest.json')),
                      website_artifact=dict(kind='desktop', format='nsis', platform='Windows', architecture='x86_64',
                                            filename=installer.name, sha256=controller.digest(installer)))
        record = self.root / 'result.json'
        record.write_text(json.dumps(result))
        log = self.root / 'compiler.jsonl'
        log.write_text(json.dumps(dict(reason='compiler-artifact', target=dict(name='windows_desktop'),
                                       profile=dict(test=True), executable=str(harness))) + '\n')
        with patch.object(controller, 'REPO', self.root), \
             patch.object(controller.subprocess, 'check_output', return_value=sha):
            output = self.root / 'desktop.zip'
            exported = controller.bundle_desktop(record, log, output)
            self.assertEqual(exported['native_acceptance'], 'not_executed')
            with zipfile.ZipFile(output) as archive:
                manifest = json.loads(archive.read('manifest.json'))
                self.assertEqual(manifest['installer_format'], 'nsis')
                self.assertEqual(archive.read('hirouted.exe'), b'embedded-hirouted.exe')
            installer.write_bytes(b'changed installer')
            with self.assertRaisesRegex(ValueError, 'measured build identity'):
                controller.bundle_desktop(record, log, self.root / 'changed.zip')
            self.assertFalse((self.root / 'changed.zip').exists())
            installer.write_bytes(b'component-nsis-fixture')
            (payload / 'hirouted.exe').write_bytes(b'changed daemon')
            with self.assertRaisesRegex(ValueError, 'Embedded product bytes'):
                controller.bundle_desktop(record, log, self.root / 'changed-product.zip')

    def test_symlink_and_multiple_links_are_not_runner_storage(self):
        archive = self.archive()
        value = host.stage(self.root, archive, controller.digest(archive))
        directory = self.root / 'bundles' / value['bundle']
        os.link(directory / 'sample.exe', self.root / 'second.exe')
        with self.assertRaises(ValueError):
            host.manifest(directory)
        (self.root / 'linked').symlink_to(directory, target_is_directory=True)
        with self.assertRaises(ValueError):
            host.plain(self.root / 'linked/sample.exe')

    def test_config_contains_paths_not_tokens_and_lives_in_home(self):
        identity = self.root / 'key'
        identity.touch()
        value = dict(version=1, host='win-alias', identity_file=str(identity),
                     python='C:/Python/python.exe', root='C:/Validation', user='acceptance',
                     session_id=2, environment={'HIROUTE_WORKER_NODE': 'C:/Node/node.exe'}, path=[])
        config = self.root / 'config.json'
        config.write_text(json.dumps(value))
        config.chmod(0o600)
        with patch.object(controller.Path, 'home', return_value=self.root):
            self.assertEqual(controller.configuration(config)['host'], 'win-alias')
            value['environment']['API_TOKEN'] = 'forbidden'
            config.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                controller.configuration(config)
        with patch.object(controller.Path, 'home', return_value=self.root / 'other'):
            with self.assertRaises(ValueError):
                controller.configuration(config)

    def test_identifiers_and_paths_cannot_change_task_or_bundle_scope(self):
        for value in ('../foreign', 'a;whoami', 'a' * 31, '-bad'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                host.task_name(value)
        for value in ('relative', 'C:/root/../foreign', 'C:/root\nforeign'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                controller.windows_path(value)

    def test_shell_payload_preserves_quotes_without_interpolation(self):
        value = "C:/space ' $env:PATH ; directory/file.py"
        script = '& ' + controller.ps_literal(value)
        command = controller.powershell(script)
        self.assertEqual(base64.b64decode(command.split()[-1]).decode('utf-16le'), script)
        self.assertEqual(controller.ps_literal(value), "'C:/space '' $env:PATH ; directory/file.py'")

    def test_unknown_task_outcome_retains_owner_lock(self):
        archive = self.archive()
        value = host.stage(self.root, archive, controller.digest(archive))
        request = dict(run_id='a' * 32, config=dict(python='C:/python.exe', user='acceptance'),
                       harness=value['bundle'], product=value['bundle'], binary='sample.exe',
                       case='selected_test', timeout=30, driver_path='C:/runner.py')
        with patch.object(host, 'private_runs'), patch.object(host, 'ps', side_effect=RuntimeError('unknown submission')):
            with self.assertRaises(RuntimeError):
                host.start(self.root, request)
            with self.assertRaises(FileExistsError):
                host.start(self.root, dict(request, run_id='b' * 32))
        self.assertEqual(json.loads((self.root / 'active.json').read_text())['run_id'], 'a' * 32)

    def test_recovery_refuses_active_or_foreign_configuration(self):
        run_id = 'a' * 32
        run = self.root / 'runs' / run_id
        run.mkdir(parents=True)
        request = dict(case='actual_windows_complete_public_headless_business',
                       config=dict(user='acceptance'))
        (run / 'request.json').write_text(json.dumps(request))
        home = self.root / 'home'
        (home / '.claude').mkdir(parents=True)
        settings = home / '.claude/settings.json'
        settings.write_text('{"daily_setting":true}')
        (run / 'result.json').write_text('{"passed":false}')
        with patch.object(host, 'task_state', return_value=dict(state='absent')), \
             patch.object(host.Path, 'home', return_value=home), \
             patch.dict(os.environ, USERNAME='acceptance', LOCALAPPDATA=str(home / 'local')):
            with self.assertRaisesRegex(ValueError, 'untouched disposable marker'):
                host.recover(self.root, run_id)
            self.assertTrue(settings.exists())
            (self.root / 'active.json').write_text('{}')
            with self.assertRaisesRegex(ValueError, 'Finish the stopped run'):
                host.recover(self.root, run_id)
            (self.root / 'active.json').unlink()
            settings.write_text('{"windows_headless_preserved":true}')
            (run / 'result.json').touch()
            value = host.recover(self.root, run_id)
            self.assertEqual(value['preserved'], ['recovered-claude-settings.json'])
            self.assertFalse(settings.exists())

    def test_task_success_without_case_result_is_failure(self):
        (self.root / 'runs' / ('a' * 32)).mkdir(parents=True)
        with patch.object(host, 'task_state', return_value=dict(state='Ready', result=0)):
            self.assertEqual(host.status(self.root, 'a' * 32)['state'], 'failed')

    def test_interrupted_gateway_recovery_preserves_only_its_verified_fixture(self):
        home = self.root / 'home'
        data = home / '.local/share/hiroute'
        install = data / 'Windows headless 测试 123'
        stable = home / '.local/bin'
        state = home / 'local/HiRoute/cli'
        run = self.root / 'runs' / ('a' * 32)
        for path in (install, stable, state, run):
            path.mkdir(parents=True)
        request = dict(run_id='a' * 32, harness='b' * 12 + '-' + 'c' * 12,
                       product='d' * 12 + '-' + 'e' * 12, binary='windows_headless.exe',
                       config=dict(user='acceptance', session_id=2),
                       case='windows_gateway_foreground_recovery::actual_windows_foreground_gateway_recovery')
        (run / 'request.json').write_text(json.dumps(request))
        (run / 'result.json').write_text('{"passed":false,"error_type":"TimeoutError"}')
        marker = data / 'standalone.json'
        marker.write_text(json.dumps(dict(schema_version='hiroute.standalone-install/v1',
                         target='x86_64-pc-windows-msvc', install_root=str(install),
                         service_definition=str(install / 'fixture-task.xml'))))
        os.utime(run / 'request.json', (time.time() - 5,) * 2)
        files = {}
        for name in ('hiroute.exe', 'hirouted.exe'):
            image = install / name
            image.write_bytes(name.encode())
            os.link(image, stable / name)
            files[name] = dict(sha256=host.digest(image))
        (run / 'result.json').touch()
        with patch.object(host, 'task_state', return_value=dict(state='absent')), \
             patch.object(host.Path, 'home', return_value=home), \
             patch.dict(os.environ, USERNAME='acceptance', LOCALAPPDATA=str(home / 'local')), \
             patch.object(host, 'manifest', return_value=dict(files=files)), \
             patch.object(host, 'ps', side_effect=['[]', '1', '[456]', '0']), \
             patch.object(host, 'stop_interrupted_daemon') as stop:
            with self.assertRaisesRegex(ValueError, 'still has live processes'):
                host.recover(self.root, 'a' * 32)
            self.assertTrue(marker.exists())
            self.assertTrue((stable / 'hirouted.exe').exists())
            value = host.recover(self.root, 'a' * 32)
            stop.assert_called_once_with(456, install / 'hirouted.exe', request['config'],
                                        (run / 'request.json').stat().st_mtime,
                                        (run / 'result.json').stat().st_mtime)
        self.assertFalse(marker.exists())
        self.assertFalse((stable / 'hirouted.exe').exists())
        self.assertEqual((run / 'interrupted-installation/hirouted.exe').read_bytes(), b'hirouted.exe')
        self.assertEqual(len(value['preserved']), 3)

    def test_evidence_download_uses_sftp_path_syntax(self):
        self.assertEqual(controller.remote_evidence_path(
            r'C:\Validation\runs\sample', 'result.json'),
            'C:/Validation/runs/sample/result.json')

    def test_active_runtime_stop_requires_selected_panic_and_owner(self):
        run_id = 'a' * 32
        run = self.root / 'runs' / run_id
        run.mkdir(parents=True)
        request = dict(binary='windows_headless.exe', config=dict(user='acceptance'),
                       case='actual_windows_public_headless_business_without_claude')
        (run / 'request.json').write_text(json.dumps(request))
        (self.root / 'active.json').write_text(json.dumps(dict(run_id=run_id)))
        (run / 'stderr.log').write_text('ordinary progress')
        with patch.object(host, 'task_state', return_value=dict(state='Running')), \
             patch.dict(os.environ, USERNAME='acceptance'), \
             patch.object(host, 'recover_interrupted_fixture') as recover:
            with self.assertRaisesRegex(ValueError, 'has not reported a panic'):
                host.stop_panicking_runtime(self.root, run_id)
            recover.assert_not_called()
            (run / 'stderr.log').write_text("thread '" + request['case'] + "' (123) panicked at fixture.rs:1:1:\n")
            host.stop_panicking_runtime(self.root, run_id)
            recover.assert_called_once_with(self.root, run, request, stop_only=True)
            (run / 'result.json').write_text('{"passed":false}')
            with self.assertRaisesRegex(ValueError, 'active nonterminal run'):
                host.stop_panicking_runtime(self.root, run_id)


if __name__ == '__main__':
    unittest.main()
