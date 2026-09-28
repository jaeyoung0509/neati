import json
import unittest
from pathlib import Path

from cleanup_gap_ledger import build_ledger, classify


class GapLedgerTests(unittest.TestCase):
    def test_committed_snapshot_accounts_for_every_unmatched_row(self):
        report = json.loads((Path(__file__).resolve().parents[1] / "docs/evidence/cleanup-coverage-329.json").read_text())
        self.assertEqual(len(report["rows"]), 38)
        self.assertEqual(len({row["candidate_id"] for row in report["rows"]}), 38)
        self.assertEqual(report["classification_counts"], {"candidate": 5, "protected": 8, "unresolved": 25})
        self.assertEqual(report["zero_preview_rows"], 11)
        for row in report["rows"]:
            self.assertIsNone(row["verified_reclaim_bytes"])
            self.assertEqual(row["execution_policy"], "not_authorized_by_this_report")
        self.assertNotIn("/Users/", json.dumps(report))

    def test_unknown_private_names_never_leak(self):
        rows = [{"path": "/Users/private-person/projects/secret-client", "displayed_bytes": 123, "section": "private"}]
        result = build_ledger(rows, [], "/Users/private-person")
        self.assertNotIn("private-person", json.dumps(result))
        self.assertNotIn("secret-client", json.dumps(result))
        self.assertEqual(result["rows"][0]["classification"], "unresolved")
        self.assertIsNone(result["rows"][0]["verified_reclaim_bytes"])

    def test_known_does_not_authorize(self):
        result = classify("/fixture/Library/Caches/com.apple.python/Users", "/fixture", 4096)
        self.assertEqual(result["classification"], "candidate")
        self.assertEqual(result["confidence"], "source_review")

    def test_zero_does_not_erase_protection(self):
        result = classify("/fixture/Library/Logs/DiagnosticReports", "/fixture", 0)
        self.assertTrue(result["preview_zero"])
        self.assertEqual(result["classification"], "protected")

    def test_shell_machine_name_not_exported(self):
        result = classify("/fixture/.zcompdump-secret-host-5.9.zwc", "/fixture", 4096)
        self.assertEqual(result["label"], "Compiled shell completion dump")
        self.assertNotIn("secret-host", json.dumps(result))

    def test_only_reference_only_rows_are_exported(self):
        reference = [{"path": p, "displayed_bytes": 4096, "section": "test"} for p in ("/fixture/cache", "/fixture/cache/child", "/fixture/other")]
        zenith = [{"path": "/fixture/cache", "eligibility": "reviewable"}]
        result = build_ledger(reference, zenith, "/fixture")
        self.assertEqual(len(result["rows"]), 1)
        self.assertEqual(result["relationships"], {"exact": 1, "zenith_ancestor": 1, "reference_only": 1})

    def test_nested_preview_is_flagged_and_order_is_stable(self):
        reference = [{"path": p, "displayed_bytes": 4096, "section": "test"} for p in ("/fixture/other", "/fixture/other/child")]
        result = build_ledger(reference, [], "/fixture")
        self.assertEqual(sum(row["overlaps_another_preview"] for row in result["rows"]), 1)
        self.assertEqual(result["rows"], build_ledger(list(reversed(reference)), [], "/fixture")["rows"])

    def test_home_prefix_is_component_bounded(self):
        result = classify("/fixture-other/Library/Caches/com.apple.python/Users", "/fixture", 4096)
        self.assertEqual(result["classification"], "unresolved")


if __name__ == "__main__":
    unittest.main()
