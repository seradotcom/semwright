import unittest
from collector import parse_tests
EXPECTED = {"authoring": 12, "authoring_store": 11}
def output(color=False, passed=11, ignored=0):
    header = "\x1b[1m\x1b[92m     Running\x1b[0m" if color else "     Running"
    return (f"{header} tests/authoring.rs (target/debug/deps/authoring-hash)\n"
        "test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        f"{header} tests/authoring_store.rs (target/debug/deps/authoring_store-hash)\n"
        f"test result: ok. {passed} passed; 0 failed; {ignored} ignored; 0 measured; 0 filtered out;\n")
class CollectorTests(unittest.TestCase):
    def test_real_cargo_color_framing_is_equivalent(self):
        self.assertEqual(parse_tests(output(False), EXPECTED), parse_tests(output(True), EXPECTED))
        self.assertEqual(sum(v["executed"] for v in parse_tests(output(True), EXPECTED).values()), 23)
    def test_missing_zero_ignored_failed_and_duplicate_results_fail_closed(self):
        cases = ["", output().split("     Running tests/authoring_store")[0], output(passed=0),
            output(ignored=1), output().replace("ok. 11 passed; 0 failed", "FAILED. 10 passed; 1 failed"),
            output()+"test result: ok. 11 passed; 0 failed; 0 ignored;\n", output()+output()]
        for case in cases:
            with self.subTest(case=case), self.assertRaises(ValueError): parse_tests(case, EXPECTED)
if __name__ == "__main__": unittest.main()
