"""Run the full frozen case suite against its hand-written expectations."""
import unittest

from cases import CASES, execute


class CaseSuiteTests(unittest.TestCase):
    def test_every_registered_case_matches_expectation(self):
        failures = []
        for spec in CASES:
            actual = execute(spec)
            if actual != spec.expected:
                failures.append((spec.id, spec.expected, actual))
        self.assertFalse(failures, failures)
        self.assertGreaterEqual(len(CASES), 30, "case suite thinned")

    def test_tampered_report_is_rejected(self):
        import report as report_module
        value = report_module.build_report()
        value["cases"][0]["actual"] = {"status": "forged"}
        with self.assertRaises(SystemExit):
            report_module.validate(value)


if __name__ == "__main__":
    unittest.main()
