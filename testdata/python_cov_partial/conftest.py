import json
from pathlib import Path


def pytest_sessionfinish(session, exitstatus):
    root = Path(__file__).resolve().parent
    report = root / ".sc" / "coverage" / "pytest.json"
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(
        json.dumps(
            {
                "files": {
                    "app.py": {
                        "executed_lines": [],
                        "missing_lines": list(range(1, 15)),
                    }
                },
                "totals": {"percent_covered": 0.0},
            }
        ),
        encoding="utf-8",
    )
