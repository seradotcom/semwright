"""Parse Cargo/libtest framing; terminal formatting is not test evidence."""
import re
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
HEADER = re.compile(r"^\s*Running tests/([a-z0-9_]+)\.rs[^\n]*\n", re.M)
RESULT = re.compile(r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", re.M)
def parse_tests(log, expected):
    plain = ANSI.sub("", log)
    headers = list(HEADER.finditer(plain))
    names = [h.group(1) for h in headers]
    if len(names) != len(expected) or set(names) != set(expected):
        raise ValueError("Missing, duplicate or unexpected test binary")
    counts = {}
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(plain)
        results = RESULT.findall(plain[header.end():end])
        if len(results) != 1:
            raise ValueError("Exactly one libtest result is required per binary")
        status, passed, failed, ignored = results[0]
        passed, failed, ignored = int(passed), int(failed), int(ignored)
        name = header.group(1)
        if status != "ok" or failed != 0 or ignored != 0 or passed < expected[name]:
            raise ValueError("Failed, ignored or missing required tests in " + name)
        counts[name] = {"executed": passed, "ignored": ignored}
    return counts
