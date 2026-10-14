#!/usr/bin/env python3

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("public_surface.py")
SPEC = importlib.util.spec_from_file_location("public_surface", SCRIPT)
public_surface = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(public_surface)


class PublicSurfaceTests(unittest.TestCase):
    def setUp(self):
        self.manifest = public_surface.load_manifest(
            SCRIPT.parents[1] / "references/public-surface.v1.json"
        )

    def test_classifies_product_policy_and_unknown_paths(self):
        groups = public_surface.classify(
            [
                "crates/gateway/src/lib.rs",
                "docs/macos-installation.md",
                "AGENTS.md",
                "issue-spec/specs/routing/spec.md",
                "docs/new-design.md",
                "new-top-level/file.txt",
            ],
            self.manifest,
        )
        self.assertEqual(
            groups,
            {
                "public": ["crates/gateway/src/lib.rs", "docs/macos-installation.md", "AGENTS.md", "issue-spec/specs/routing/spec.md"],
                "private": [],
                "unknown": ["docs/new-design.md", "new-top-level/file.txt"],
            },
        )

    def test_text_audit_reports_rule_names_without_secret_values(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text(
                "private https://fixture.example." + "internal and token ghp_" + "a" * 24,
                encoding="utf-8",
            )
            findings = public_surface.text_findings(root)
        self.assertEqual(
            findings,
            [
                {"path": "README.md", "rule": "github-token"},
                {"path": "README.md", "rule": "private-host"},
            ],
        )
        self.assertNotIn("ghp_", json.dumps(findings))

    def test_required_public_files_are_enforced(self):
        with self.assertRaises(public_surface.SurfaceError):
            public_surface.validate_required({"README.md"}, self.manifest)

    def test_publishing_a_rename_removes_the_old_public_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git = lambda *args: public_surface.run_git(root, *args)
            git("init", "--quiet")
            git("config", "user.name", "Publication Fixture")
            git("config", "user.email", "fixture@example.test")
            git("config", "commit.gpgsign", "false")
            git("config", "core.hooksPath", "/dev/null")
            old = root / "crates/gateway/src/old.rs"
            new = old.with_name("new.rs")
            old.parent.mkdir(parents=True)
            old.write_text("// unchanged native implementation\n" * 40)
            git("add", ".")
            git("commit", "--quiet", "-m", "Original public file")
            base = git("rev-parse", "HEAD").decode().strip()
            old.rename(new)
            git("add", "-A")
            git("commit", "--quiet", "-m", "Rename public owner")
            head = git("rev-parse", "HEAD").decode().strip()

            paths = public_surface.changed_paths(root, base, head)
            selected = public_surface.classify(paths, self.manifest)["public"]
            patch = git("diff", "--binary", base, head, "--", *selected)
            git("checkout", "--quiet", "--detach", base)
            patch_file = root / "publication.patch"
            patch_file.write_bytes(patch)
            git("apply", str(patch_file))
            self.assertTrue(new.is_file())
            self.assertFalse(old.exists(), "publication left the removed owner behind")

    def test_retired_docs_keep_deletion_without_reopening_unclassified_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git = lambda *args: public_surface.run_git(root, *args)
            git("init", "--quiet")
            git("config", "user.name", "Publication Fixture")
            git("config", "user.email", "fixture@example.test")
            git("config", "commit.gpgsign", "false")
            git("config", "core.hooksPath", "/dev/null")
            manifest = {**self.manifest, "include_files": [],
                        "include_docs": ["docs/retired.md", "docs/unlisted.md"]}
            manifest_path = root / "surface.json"
            manifest_path.write_text(json.dumps(manifest))
            (root / "docs").mkdir()
            retired = root / "docs/retired.md"
            retired.write_text("Old public navigation\n")
            unlisted = root / "docs/unlisted.md"
            unlisted.write_text("Previously classified text\n")
            git("add", ".")
            git("commit", "--quiet", "-m", "Original public documentation")
            base = git("rev-parse", "HEAD").decode().strip()

            retired.unlink()
            unlisted.write_text("Changed content without current classification\n")
            (root / "docs/new.md").write_text("New unclassified document\n")
            manifest["include_docs"] = []
            manifest_path.write_text(json.dumps(manifest))
            git("add", "-A")
            git("commit", "--quiet", "-m", "Consolidate documentation")
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--repository", str(root),
                 "--manifest", str(manifest_path), "classify-range",
                 "--base", base, "--head", "HEAD"],
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            groups = json.loads(result.stdout)
            self.assertEqual(groups["public"], ["docs/retired.md"])
            self.assertIn("docs/new.md", groups["unknown"])
            self.assertIn("docs/unlisted.md", groups["unknown"])
            public_surface.validate_required({"docs/unlisted.md"}, manifest)

            patch = git("diff", "--binary", base, "HEAD", "--", *groups["public"])
            git("checkout", "--quiet", "--detach", base)
            patch_file = root / "publication.patch"
            patch_file.write_bytes(patch)
            git("apply", str(patch_file))
            self.assertFalse(retired.exists())
            self.assertEqual(unlisted.read_text(), "Previously classified text\n")
            self.assertFalse((root / "docs/new.md").exists())

    def fixture(self, root):
        git = lambda *args: public_surface.run_git(root, *args)
        git('init', '--quiet')
        git('config', 'user.name', 'Publication Fixture')
        git('config', 'user.email', 'fixture@example.test')
        git('config', 'commit.gpgsign', 'false')
        git('config', 'core.hooksPath', '/dev/null')
        git('commit', '--allow-empty', '--quiet', '-s', '-m', 'Public baseline')
        return git, git('rev-parse', 'HEAD').decode().strip()

    def range_audit(self, root, base):
        manifest = {**self.manifest, 'include_files': [], 'include_docs': []}
        return public_surface.audit_range(root, base, 'HEAD', manifest, root / 'absent-manifest.json')

    def test_intermediate_contents_paths_messages_and_signoffs_are_all_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git, base = self.fixture(root)
            (root / 'crates').mkdir()
            (root / 'artifacts').mkdir()
            secret = 'ghp_' + 'x' * 30
            (root / 'crates/example.txt').write_text(secret)
            (root / 'artifacts/raw.txt').write_text('raw investigation')
            git('add', '.')
            git('commit', '--quiet', '-m', 'Endpoint https://fixture.example.' + 'internal/path')
            exposed = git('rev-parse', 'HEAD').decode().strip()
            (root / 'crates/example.txt').unlink()
            (root / 'artifacts/raw.txt').unlink()
            git('add', '-A')
            git('commit', '--quiet', '-s', '-m', 'Remove intermediate files')
            result = self.range_audit(root, base)
            rules = {finding['rule'] for finding in result['findings']}
            self.assertTrue({'github-token', 'private-path', 'private-host', 'author-signoff'} <= rules)
            self.assertTrue(any(f['commit'] == exposed and f['rule'] == 'github-token' for f in result['findings']))
            self.assertNotIn(secret, json.dumps(result))
            self.assertEqual(result['commits'], 2)

    def test_merge_side_history_cannot_hide_an_added_then_removed_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git, base = self.fixture(root)
            main = git('symbolic-ref', '--short', 'HEAD').decode().strip()
            git('checkout', '--quiet', '-b', 'side')
            (root / 'crates').mkdir()
            path = root / 'crates/temporary.txt'
            path.write_text('sk-or-v1-' + 'a' * 30)
            git('add', '.')
            git('commit', '--quiet', '-s', '-m', 'Add temporary content')
            exposed = git('rev-parse', 'HEAD').decode().strip()
            path.unlink()
            git('add', '-A')
            git('commit', '--quiet', '-s', '-m', 'Remove temporary content')
            git('checkout', '--quiet', main)
            git('merge', '--quiet', '--no-ff', '--signoff', 'side', '-m', 'Merge side change')
            result = self.range_audit(root, base)
            self.assertTrue(any(f['commit'] == exposed and f['rule'] == 'model-api-key' for f in result['findings']))

    def test_public_policy_and_optional_personal_deny_patterns(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git, base = self.fixture(root)
            (root / '.agents/skills/example').mkdir(parents=True)
            (root / '.agents/skills/example/SKILL.md').write_text('Public shared instructions')
            git('add', '.')
            git('commit', '--quiet', '-s', '-m', 'Add public skill')
            self.assertEqual(self.range_audit(root, base)['findings'], [])
            raw = b'Example endpoint https://private-history.example.test'
            found = public_surface.scan(raw, 'body.md', ['private-history.example.test'])
            self.assertEqual(found, [{'path': 'body.md', 'rule': 'personal-deny-pattern'}])
            self.assertNotIn('private-history', json.dumps(found))

    def test_ignored_video_skill_cannot_be_forced_into_public_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git, base = self.fixture(root)
            skill = root / '.agents/skills/hiroute-promo-video'
            skill.mkdir(parents=True)
            (skill / 'SKILL.md').write_text('Private optional video instructions\n')
            (skill / 'scripts').mkdir()
            (skill / 'scripts/render.py').write_text('# Private optional helper\n')
            link = root / '.claude/skills/hiroute-promo-video'
            link.parent.mkdir(parents=True)
            link.symlink_to('../../.agents/skills/hiroute-promo-video')
            (root / '.gitignore').write_text(
                '/.agents/skills/hiroute-promo-video/\n'
                '/.claude/skills/hiroute-promo-video\n')
            private_paths = [str(path.relative_to(root)) for path in
                             (skill / 'SKILL.md', skill / 'scripts/render.py', link)]
            git('add', '.gitignore')
            git('add', '--force', *private_paths)
            git('commit', '--quiet', '-s', '-m', 'Accidentally stage an ignored skill')
            exposed = git('rev-parse', 'HEAD').decode().strip()
            git('rm', '--quiet', '-r', '--', str(skill.relative_to(root)),
                str(link.relative_to(root)))
            git('commit', '--quiet', '-s', '-m', 'Remove the private skill')
            result = self.range_audit(root, base)
            blocked = {finding['path'] for finding in result['findings']
                       if finding['rule'] == 'private-path' and finding['commit'] == exposed}
            self.assertEqual(blocked, set(private_paths))


if __name__ == "__main__":
    unittest.main()
