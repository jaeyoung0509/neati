import unittest

from cleanup_coverage_audit import compare, displayed_bytes


class CoverageUnitsTests(unittest.TestCase):
    def test_decimal_reference_units_are_not_binary(self):
        for label, expected in [("1 KB", 1000), ("625.2 MB", 625200000),
                                ("1.25 GB", 1250000000), ("1 TB", 10**12),
                                ("0 B", 0), ("4.1 MB, 3 files", 4100000)]:
            with self.subTest(label=label):
                self.assertEqual(displayed_bytes(label), expected)

    def test_explicit_binary_units_remain_distinct(self):
        self.assertEqual(displayed_bytes("1 MiB"), 1048576)
        self.assertEqual(displayed_bytes("1 KiB"), 1024)
        self.assertEqual(displayed_bytes("1 GiB"), 1073741824)

    def test_unknown_is_not_zero(self):
        for label in ["unknown", "", "-1 MB", "nan MB", "1 PB"]:
            self.assertIsNone(displayed_bytes(label))

    def test_nested_potential_is_not_added_to_unique_total(self):
        rows = [{"path": path, "section": "test", "displayed_bytes": size}
                for path, size in [("/fixture/cache", 1000000),
                                   ("/fixture/cache/child", 400000)]]
        *_, overlap = compare(rows, [], "/fixture", False)
        self.assertEqual(overlap["top_level_rounded_preview_bytes"], 1000000)


if __name__ == "__main__":
    unittest.main()
