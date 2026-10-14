from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("materialize_client_metadata.py")
SPEC = importlib.util.spec_from_file_location("materialize_client_metadata", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
materializer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(materializer)


class MaterializeClientMetadataTests(unittest.TestCase):
    def scan(self):
        return {
            "artifact_kind": "hiroute.model-metadata-client-discovery",
            "policy": {"target_code_executed": False},
            "profile": {"profile_key": "fixture"},
            "repository": {"commit": "a" * 40},
            "candidates": {
                "providers": [
                    {
                        "provider_id": "example",
                        "assertions": [
                            {
                                "api": "openai-completions",
                                "baseUrl": "https://api.example.test/v1",
                                "defaultModel": "org/example-2",
                            }
                        ],
                        "locations": [
                            {"path": "plugin.json", "pointer": "/providers/example"}
                        ],
                    }
                ],
                "models": [
                    {
                        "provider_id": "example",
                        "model_id": "org/example-2",
                        "roles": ["client-catalog"],
                        "catalog_lexical_matches": [],
                        "assertions": [
                            {
                                "id": "org/example-2",
                                "name": "Example 2",
                                "contextWindow": 1000,
                                "maxTokens": 100,
                                "input": ["text", "image"],
                                "reasoning": True,
                            }
                        ],
                        "locations": [
                            {"path": "plugin.json", "pointer": "/providers/example/models/0"}
                        ],
                    },
                    {
                        "provider_id": "model-list-only",
                        "model_id": "plain-model",
                        "roles": ["client-static-collection"],
                        "catalog_lexical_matches": [],
                        "assertions": [],
                        "locations": [{"path": "catalog.py", "line": 4}],
                    },
                ],
            },
        }

    def test_derives_scenario_eligibility_without_touching_models(self) -> None:
        catalog = {"models": [{"model_key": "canonical"}]}
        result = materializer.materialize(catalog, [self.scan()])
        self.assertEqual(result["models"], catalog["models"])
        self.assertEqual(len(result["provider_metadata_records"]), 2)
        self.assertEqual(len(result["model_metadata_records"]), 2)
        provider = next(
            item
            for item in result["provider_metadata_records"]
            if item["provider_id"] == "example"
        )
        self.assertEqual(provider["protocol_candidates"], ["chat_completions"])
        self.assertEqual(provider["metadata_completeness"]["endpoint"], "complete")
        self.assertIn("custom-api-endpoint-prefill", provider["usable_for"])
        self.assertEqual(
            provider["provenance_refs"],
            [{"kind": "client-discovery-run", "source_key": "fixture"}],
        )
        model = next(
            item
            for item in result["model_metadata_records"]
            if item["provider_id"] == "example"
        )
        self.assertEqual(model["model_record_key"], "fixture/example/org%2Fexample-2")
        self.assertEqual(model["context_tokens"]["value"], 1000)
        self.assertEqual(model["capability_hints"]["vision"], "supported")
        self.assertEqual(model["metadata_completeness"]["limits"], "complete")
        self.assertIn("request-limit-prefill", model["usable_for"])

    def test_materialization_is_deterministic(self) -> None:
        first = materializer.materialize({}, [self.scan()])
        second = materializer.materialize({}, [self.scan()])
        self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()
