#!/usr/bin/env python3
"""Verify baseline package binding before native administrator extraction."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'),
                                                Path(__file__).with_name(name + '.py'))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


staging = load('windows-ci-package')
packager = load('package-standalone')
pe = load('test-windows-native-artifacts')
CANDIDATE = 'a' * 40
BASELINE = 'b' * 40


class RestagedBaseline(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.calls = []
        self.extractions = []
        self.head = CANDIDATE
        self.args = SimpleNamespace(expected_sha=CANDIDATE, package_revision=BASELINE,
                                    output=self.root / 'baseline-trust',
                                    manifest=self.root / 'baseline.json',
                                    archive=self.root / 'baseline.tar.gz')
        self.addCleanup(patch.stopall)
        patch.object(staging.platform, 'system', return_value='Windows').start()
        patch.object(staging, 'command', side_effect=self.command).start()
        patch.object(staging, 'module', return_value=packager).start()
        patch.object(staging, 'trusted_extraction', side_effect=self.extract).start()
        self.package(BASELINE)

    def command(self, *args):
        self.calls.append(args)
        if args == ('git', 'rev-parse', 'HEAD'):
            return self.head
        if args == ('git', 'diff', '--exit-code', 'HEAD', '--'):
            return ''
        self.fail('Unexpected command before package verification: ' + repr(args))

    def extract(self, owner, manifest, archive, destination):
        # ACL application belongs to actual Windows acceptance. These tests
        # exercise the real closed archive verifier before that boundary.
        verified = owner.verify(manifest, archive)
        destination.mkdir(parents=True, exist_ok=False)
        for name, (payload, _) in owner.safe_members(archive, verified['files']).items():
            path = destination / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
        self.extractions.append(destination)
        return verified

    def package(self, revision, target='x86_64-pc-windows-msvc'):
        files = {}
        executable = packager.executable_names(target)
        for index, name in enumerate(sorted(packager.expected_files(target))):
            path = self.root / ('input-' + str(index))
            path.write_bytes(pe.small_pe() if name in executable.values() else b'closed package fixture')
            files[name] = path
        packager.deterministic_archive(self.args.archive, files)
        inventory = {name: {'sha256': packager.digest(path), 'size': path.stat().st_size,
                           'mode': 'executable' if name in executable.values() else 'read_only'}
                     for name, path in files.items()}
        manifest = {'schema': packager.SCHEMA, 'version': '0.3.1', 'revision': revision,
                    'target': target, 'distribution': 'integration-candidate', 'files': inventory,
                    'archive': {'filename': self.args.archive.name,
                                'sha256': packager.digest(self.args.archive),
                                'size': self.args.archive.stat().st_size},
                    'cpa': {'version': 'fixture', 'sha256': inventory[executable['cpa']]['sha256']}}
        self.args.manifest.write_text(json.dumps(manifest))

    def test_separate_baseline_revision_keeps_current_consumer_and_verified_original_bytes(self):
        result = staging.restage(self.args)
        self.assertEqual(result['source_revision'], BASELINE)
        self.assertEqual(result['consumer_revision'], CANDIDATE)
        self.assertEqual(result['production_runtime'], 'not_executed')
        self.assertEqual(self.extractions, [self.args.output / 'administrator-package'])
        self.assertEqual(Path(result['administrator_cli']).read_bytes(), pe.small_pe())

    def test_unbound_old_package_never_reaches_administrator_extraction(self):
        self.args.package_revision = None
        with self.assertRaisesRegex(ValueError, 'explicitly bound'):
            staging.restage(self.args)
        self.assertEqual(self.extractions, [])
        self.assertFalse(self.args.output.exists())

    def test_current_checkout_cannot_be_replaced_by_baseline_checkout(self):
        self.head = BASELINE
        with self.assertRaisesRegex(ValueError, 'candidate checkout'):
            staging.restage(self.args)
        self.assertEqual(self.extractions, [])

    def test_baseline_revision_is_exact_and_cannot_accept_another_valid_package(self):
        for unexpected in ['c' * 40, 'b' * 12, 'B' * 40]:
            self.args.package_revision = unexpected
            with self.assertRaises(ValueError):
                staging.restage(self.args)
        self.assertEqual(self.extractions, [])

    def test_tampered_complete_archive_is_rejected_before_extraction(self):
        self.args.archive.write_bytes(self.args.archive.read_bytes() + b'foreign tail')
        with self.assertRaisesRegex(ValueError, 'archive digest'):
            staging.restage(self.args)
        self.assertEqual(self.extractions, [])


if __name__ == '__main__':
    unittest.main()
