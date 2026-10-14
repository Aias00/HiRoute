from __future__ import annotations

import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("scan_client_repo.py")
SPEC = importlib.util.spec_from_file_location("scan_client_repo", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
scanner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scanner)


def run(*args: str, cwd: Path) -> str:
    result = subprocess.run(args, cwd=cwd, check=True, capture_output=True, text=True)
    return result.stdout.strip()


class ScanClientRepoTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.repo = self.root / "client"
        self.repo.mkdir()
        run("git", "init", "-q", cwd=self.repo)
        run("git", "remote", "add", "origin", "git@github.com:example/client.git", cwd=self.repo)
        manifest = {
            "modelCatalog": {
                "providers": {
                    "example": {
                        "api": "openai-completions",
                        "baseUrl": "https://api.example.test/v1",
                        "defaultModel": "example-2",
                        "models": [
                            {
                                "id": "example-2",
                                "name": "Example 2",
                                "contextWindow": 1000,
                                "maxTokens": 100,
                            }
                        ],
                    }
                }
            }
        }
        manifest_path = self.repo / "extensions/example/openclaw.plugin.json"
        manifest_path.parent.mkdir(parents=True)
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        run("git", "add", ".", cwd=self.repo)
        run(
            "git",
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "-qm",
            "fixture",
            cwd=self.repo,
        )
        self.commit = run("git", "rev-parse", "HEAD", cwd=self.repo)
        self.profiles = self.root / "profiles.json"
        self.profiles.write_text(
            json.dumps(
                {
                    "artifact_kind": "hiroute.model-metadata-client-source-profiles",
                    "profiles": [
                        {
                            "profile_key": "fixture",
                            "evidence_eligible": False,
                            "expected_repository_urls": ["https://github.com/example/client"],
                            "parsers": [
                                {
                                    "kind": "openclaw-plugin-manifests",
                                    "manifest_paths": ["extensions/*/openclaw.plugin.json"],
                                    "exclude_paths": [],
                                }
                            ],
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        self.registry = self.root / "registry.json"
        self.registry.write_text(
            json.dumps({"authorities": []}), encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def build(self):
        return scanner.build_artifact(
            repo=self.repo,
            commit=self.commit,
            profile_key="fixture",
            profiles_path=self.profiles,
            source_registry_path=self.registry,
            catalog_path=None,
            as_of="2026-09-13",
        )

    def test_scan_is_deterministic_and_ignores_dirty_worktree(self) -> None:
        first = self.build()
        (self.repo / "extensions/example/openclaw.plugin.json").write_text("dirty", encoding="utf-8")
        second = self.build()
        self.assertEqual(first, second)
        self.assertEqual(first["repository"]["commit"], self.commit)
        self.assertTrue(first["policy"]["metadata_record_materialization_allowed"])
        self.assertEqual(first["policy"]["scenario_eligibility_basis"], "field-completeness")
        self.assertFalse(first["policy"]["provenance_controls_scenario_eligibility"])
        self.assertFalse(first["policy"]["working_tree_bytes_read"])
        self.assertEqual(first["counts"]["model_candidate"], 1)

    def test_rejects_moving_revision(self) -> None:
        with self.assertRaisesRegex(scanner.ScanError, "full 40-character"):
            scanner.build_artifact(
                repo=self.repo,
                commit="HEAD",
                profile_key="fixture",
                profiles_path=self.profiles,
                source_registry_path=self.registry,
                catalog_path=None,
                as_of="2026-09-13",
            )

    def test_tree_inventory_is_limited_to_profile_paths(self) -> None:
        selected = scanner.list_tree(
            self.repo,
            self.commit,
            ["extensions/*/openclaw.plugin.json"],
        )
        self.assertEqual(
            list(selected),
            ["extensions/example/openclaw.plugin.json"],
        )

    def test_locator_only_python_uses_literal_tokens(self) -> None:
        candidates = scanner.CandidateSet()
        scanner.scan_python_literal_locators(
            b'# https://comment.example.test\nURL = "https://api.example.test/v1"\n',
            "provider.py",
            "a" * 40,
            candidates,
        )
        self.assertEqual(list(candidates.locators), ["https://api.example.test/v1"])
        self.assertEqual(
            candidates.locators["https://api.example.test/v1"]["locations"][0]["line"],
            2,
        )

    def test_hermes_profile_inherits_declared_base_defaults(self) -> None:
        base_path = self.repo / "providers/base.py"
        base_path.parent.mkdir(parents=True, exist_ok=True)
        base_path.write_text(
            "from dataclasses import dataclass\n"
            "@dataclass\n"
            "class ProviderProfile:\n"
            "    name: str\n"
            "    api_mode: str = 'chat_completions'\n"
            "    auth_type: str = 'api_key'\n",
            encoding="utf-8",
        )
        plugin_path = self.repo / "plugins/model-providers/example/__init__.py"
        plugin_path.parent.mkdir(parents=True, exist_ok=True)
        plugin_path.write_text(
            "from providers.base import ProviderProfile\n"
            "example = ProviderProfile(name='example', base_url='https://api.example.test/v1')\n",
            encoding="utf-8",
        )
        run("git", "add", ".", cwd=self.repo)
        run(
            "git",
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "-qm",
            "hermes fixture",
            cwd=self.repo,
        )
        commit = run("git", "rev-parse", "HEAD", cwd=self.repo)
        self.profiles.write_text(
            json.dumps(
                {
                    "artifact_kind": "hiroute.model-metadata-client-source-profiles",
                    "profiles": [
                        {
                            "profile_key": "fixture",
                            "evidence_eligible": False,
                            "expected_repository_urls": ["https://github.com/example/client"],
                            "parsers": [
                                {
                                    "kind": "hermes-python-profiles",
                                    "profile_base_paths": ["providers/base.py"],
                                    "profile_paths": ["plugins/model-providers/*/__init__.py"],
                                    "static_catalog_paths": [],
                                    "locator_paths": [],
                                    "excluded_aggregate_paths": [],
                                }
                            ],
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        artifact = scanner.build_artifact(
            repo=self.repo,
            commit=commit,
            profile_key="fixture",
            profiles_path=self.profiles,
            source_registry_path=self.registry,
            catalog_path=None,
            as_of="2026-09-13",
        )
        provider = artifact["candidates"]["providers"][0]
        self.assertIn(
            {"api_mode": "chat_completions", "inherited_from": "ProviderProfile"},
            provider["assertions"],
        )
        self.assertIn(
            {"auth_type": "api_key", "inherited_from": "ProviderProfile"},
            provider["assertions"],
        )


if __name__ == "__main__":
    unittest.main()
