#!/usr/bin/env python3
"""Development-runner admission regressions, not native product evidence."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
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

    def test_task_success_without_case_result_is_failure(self):
        (self.root / 'runs' / ('a' * 32)).mkdir(parents=True)
        with patch.object(host, 'task_state', return_value=dict(state='Ready', result=0)):
            self.assertEqual(host.status(self.root, 'a' * 32)['state'], 'failed')

    def test_evidence_download_uses_sftp_path_syntax(self):
        self.assertEqual(controller.remote_evidence_path(
            r'C:\Validation\runs\sample', 'result.json'),
            'C:/Validation/runs/sample/result.json')


if __name__ == '__main__':
    unittest.main()
