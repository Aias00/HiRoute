#!/usr/bin/env python3
"""Ensure native verdicts and original revisions survive feedback generation."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("windows_report", Path(__file__).with_name("windows-ci-report.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class NativeFeedback(unittest.TestCase):
    def fixture(self, root, scenario="green", revision="a" * 40):
        provenance = dict(source_revision="a" * 40, host_architecture="X64", cargo="cargo fixed",
                          rustc="rustc fixed", plan="windows-headless", phase="focused", related_runs=["123"])
        (root / "provenance.json").write_text(json.dumps(provenance), encoding="utf-8-sig")
        native = dict(name="jobs", source_revision=revision, cases=["job_case"], scenario=scenario,
                      process_exit=0 if scenario == "green" else 101, plan="windows-headless", phase="focused",
                      related_runs=["123"], command=["cargo", "test", "--test", "job_windows"])
        (root / "jobs-result.json").write_text(json.dumps(native))
        state = "ok" if scenario == "green" else "FAILED"
        (root / "jobs.log").write_text("     Running tests\\job_windows.rs (target\\debug\\deps\\job.exe)\n"
                                       "running 1 test\n"
                                       f"test job_case ... {state}\n"
                                       f"test result: {state}. {int(scenario == 'green')} passed; "
                                       f"{int(scenario != 'green')} failed; 0 ignored; finished in 0.25s\n")

    def test_green_preserves_native_sha_case_and_external_ci_link(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            report = module.collect(root)
            native = json.loads((root / "feedback/jobs/validation-report.json").read_text())
            self.assertEqual(report["related_ci_runs"], ["123"])
            self.assertEqual(native["sha"], "a" * 40)
            self.assertEqual(native["scenario"], "green")
            self.assertTrue(native["observed_tests_passed"])
            self.assertEqual(native["top_modules"][0]["seconds"], .25)

    def test_red_does_not_become_green_after_derivation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, "red")
            module.collect(root)
            record = json.loads((root / "feedback/jobs/validation-report.json").read_text())
            self.assertEqual(record["process_exit"], 101)
            self.assertEqual(record["scenario"], "red")
            self.assertFalse(record["observed_tests_passed"])

    def test_foreign_candidate_case_record_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, revision="b" * 40)
            with self.assertRaisesRegex(ValueError, "another candidate"):
                module.collect(root)

    def test_failed_package_prerequisite_does_not_hide_case_reports_or_become_green(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root, "red")
            (root / "package-result.json").write_bytes(b"")
            report = module.collect(root)
            self.assertEqual(report["reports"][0]["scenario"], "red")
            self.assertEqual(report["prerequisites"], [{"name": "package-result", "result_present": False}])


if __name__ == "__main__":
    unittest.main()
