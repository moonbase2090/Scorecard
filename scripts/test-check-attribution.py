#!/usr/bin/env python3
"""Exercise the real CLI with an isolated GitHub transport, never a merge API."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
CHECK = ROOT / "scripts/check-attribution.py"
BASE = "a" * 40
HEAD = "b" * 40
MERGED = "c" * 40
FAKE_GH = r'''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
args=sys.argv[1:]
assert args[:3]==['api','--hostname','github.com'],args
endpoint=args[3]
root=Path(os.environ['ATTRIBUTION_TRANSPORT'])
fixture=json.loads((root/'api.json').read_text())
log=root/'requests.jsonl'
requests=[json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
body=sys.stdin.read() if '--input' in args else None
with log.open('a') as out:out.write(json.dumps({'endpoint':endpoint,'args':args,'body':body})+'\n')
if fixture.get('api_error'):
 print('untrusted API error must not be echoed',file=sys.stderr);sys.exit(1)
if endpoint=='user': result={'login':fixture.get('login','mb2090')}
elif '/compare/' in endpoint:
 assert '--paginate' in args and '--slurp' in args,args
 result=fixture['pages']
elif endpoint.endswith('/merge'):
 result={'merged':True,'sha':'c'*40}
elif '/git/commits/' in endpoint:
 result={'message':fixture.get('merged_message','Merge validated change\n\nExact body')}
else:
 assert endpoint=='repos/example/project/pulls/7',endpoint
 prior=sum(item['endpoint']==endpoint for item in requests)
 if prior==0 and 'replace_payload' in fixture:
  (root/'payload.json').write_text(json.dumps(fixture['replace_payload']))
 result=fixture.get('after',fixture['pr']) if prior else fixture['pr']
print(json.dumps(result))
'''


def commit(oid, message="Fix a regression"):
    return {"sha": oid, "commit": {"message": message}}


def fixture(commits=None):
    commits = commits if commits is not None else [commit(HEAD)]
    return {
        "pr": {"state": "open", "head": {"sha": HEAD}, "base": {"sha": BASE},
               "commits": len(commits), "title": "Fix a regression", "body": "Existing proof"},
        "pages": [{"total_commits": len(commits), "base_commit": {"sha": BASE}, "commits": commits}],
    }


def payload(**changes):
    value = {"sha": HEAD, "merge_method": "merge", "commit_title": "Merge validated change", "commit_message": "Exact body"}
    value.update(changes)
    return value


class AttributionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "gh").write_text(FAKE_GH)
        (self.root / "gh").chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.root) + os.pathsep + os.environ["PATH"], ATTRIBUTION_TRANSPORT=str(self.root))
        self.data = fixture()

    def run_cli(self, *args):
        (self.root / "api.json").write_text(json.dumps(self.data))
        return subprocess.run([sys.executable, str(CHECK), *args], env=self.env, capture_output=True, text=True, timeout=20)

    def pr(self):
        return self.run_cli("pr", "--repo", "example/project", "--pr", "7", "--head", HEAD)

    def merge(self, value=None, execute=False):
        (self.root / "payload.json").write_text(json.dumps(value if value is not None else payload()))
        args = ["merge", "--repo", "example/project", "--pr", "7", "--payload", str(self.root / "payload.json")]
        return self.run_cli(*args, *(["--execute"] if execute else []))

    def requests(self):
        log = self.root / "requests.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def test_prohibited_messages_fail_and_clean_message_passes(self):
        bad = ["Co-authored-by: contributor", "cO-AuThOrEd-By : contributor", "\tCO-AUTHORED-BY:\tcontributor",
               "Authored-by: assistant", "Generated-by: assistant", "Assisted-by: assistant",
               "AI-assisted-by: tool", "AI-generated-by: tool", "AI-model: tool", "AI-agent: tool",
               "Model: tool", "Agent: tool", "Generated with assistant", "🤖 Written by assistant"]
        for message in bad:
            with self.subTest(message=message):
                path = self.root / "message.txt"
                path.write_text("Fix a regression\n\n" + message + "\n")
                result = self.run_cli("message", str(path))
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("prohibited attribution", result.stderr)
        path.write_text("Fix a regression\n\nReject Co-authored-by trailers during validation.\n")
        result = self.run_cli("message", str(path))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS: message", result.stdout)

    def test_dependabot_bot_trailer_passes_but_lookalikes_fail(self):
        allowed = "Co-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>"
        path = self.root / "message.txt"
        path.write_text("Bump serde_json\n\n" + allowed + "\n")
        result = self.run_cli("message", str(path))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS: message", result.stdout)
        lookalikes = [
            allowed.replace("dependabot[bot]", "dependabot[bot]x"),
            allowed + " trailing words",
            "Co-authored-by: dependabot[bot] <49699333+dependabot[bot]@users.noreply.example.com>",
            "Co-authored-by: dependabot[bot] <dependabot[bot]@users.noreply.github.com>",
            "Co-authored-by: someone <49699333+dependabot[bot]@users.noreply.github.com>",
            "Co-authored-by: dependabot[bot]",
        ]
        for message in lookalikes:
            with self.subTest(message=message):
                path.write_text("Bump serde_json\n\n" + message + "\n")
                result = self.run_cli("message", str(path))
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("prohibited attribution", result.stderr)

    def test_historical_squash_message_is_rejected(self):
        result = self.run_cli("message", str(ROOT / "scripts/fixtures/attribution/squash-message.txt"))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("prohibited attribution", result.stderr)

    def test_bad_non_head_commit_cannot_hide_behind_clean_head(self):
        self.data = fixture([commit("d" * 40, "Fix one thing\n\nCo-authored-by: contributor"), commit(HEAD)])
        result = self.pr()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("commit " + "d" * 40, result.stderr)

    def test_pagination_beyond_250_and_late_bad_commit(self):
        commits = [commit(f"{i:040x}") for i in range(1, 301)] + [commit(HEAD)]
        self.data = fixture(commits)
        self.data["pages"] = [{"total_commits": 301, "base_commit": {"sha": BASE}, "commits": commits[i:i + 100]} for i in range(0, 301, 100)]
        result = self.pr()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS: 301 PR commit messages", result.stdout)
        self.data["pages"][2]["commits"][55]["commit"]["message"] = "Fix\n\nAI-assisted-by: tool"
        result = self.pr()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("commit " + f"{256:040x}", result.stderr)

    def test_incomplete_and_duplicate_pages_fail_closed(self):
        for pages in [[], [{"total_commits": 2, "base_commit": {"sha": BASE}, "commits": [commit(HEAD)]}],
                      [{"total_commits": 1, "base_commit": {"sha": BASE}, "commits": [commit(HEAD), commit(HEAD)]}]]:
            with self.subTest(pages=pages):
                self.data = fixture()
                self.data["pages"] = pages
                self.assertEqual(self.pr().returncode, 1)

    def test_head_base_and_text_races_fail_closed(self):
        variants = [{"head": {"sha": "e" * 40}}, {"base": {"sha": "e" * 40}}, {"body": "Edited after validation"}]
        for change in variants:
            with self.subTest(change=change):
                log = self.root / "requests.jsonl"
                log.unlink(missing_ok=True)
                self.data = fixture()
                self.data["after"] = dict(self.data["pr"], **change)
                result = self.pr()
                self.assertEqual(result.returncode, 1, result.stderr)

    def test_pr_title_and_body_are_candidate_message_inputs(self):
        for field in ("title", "body"):
            with self.subTest(field=field):
                self.data = fixture()
                self.data["pr"][field] = "Fix\n\nCo-authored-by: contributor"
                result = self.pr()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("PR " + field, result.stderr)

    def test_bad_merge_or_squash_body_prevents_submission(self):
        for method in ("merge", "squash"):
            result = self.merge(payload(merge_method=method, commit_message="Co-authored-by: contributor"), execute=True)
            self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(any(r["endpoint"].endswith("/merge") for r in self.requests()))

    def test_clean_payloads_are_validation_only_by_default(self):
        for method in ("merge", "squash"):
            result = self.merge(payload(merge_method=method, commit_message=""))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("nothing submitted", result.stdout)
        self.assertEqual(self.requests(), [])

    def test_bad_pr_commit_blocks_even_clean_proposed_merge(self):
        self.data = fixture([commit(HEAD, "Fix\n\nCo-authored-by: contributor")])
        result = self.merge(execute=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(any(r["endpoint"].endswith("/merge") for r in self.requests()))

    def test_exact_validated_payload_is_submitted_without_rereading_file(self):
        self.data["replace_payload"] = payload(commit_message="Co-authored-by: attacker")
        result = self.merge(execute=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        sent = [r for r in self.requests() if r["endpoint"].endswith("/merge")]
        self.assertEqual(len(sent), 1)
        expected = '{"sha": "' + HEAD + '", "merge_method": "merge", "commit_title": "Merge validated change", "commit_message": "Exact body"}'
        self.assertEqual(sent[0]["body"], expected)
        self.assertIn("MERGED: " + MERGED, result.stdout)
        self.assertEqual(json.loads((self.root / "payload.json").read_text())["commit_message"], "Co-authored-by: attacker")

    def test_owner_identity_and_merge_only_execution_are_enforced(self):
        self.data["login"] = "someone-else"
        self.assertEqual(self.merge(execute=True).returncode, 1)
        self.data["login"] = "mb2090"
        self.assertEqual(self.merge(payload(merge_method="squash"), execute=True).returncode, 1)
        self.assertFalse(any(r["endpoint"].endswith("/merge") for r in self.requests()))

    def test_missing_fields_unknown_options_and_wrong_head_fail(self):
        for value in [{"sha": HEAD}, payload(force=True), payload(sha="e" * 40)]:
            with self.subTest(value=value):
                result = self.merge(value, execute=True)
                self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(any(r["endpoint"].endswith("/merge") for r in self.requests()))

    def test_shell_and_workflow_commands_are_never_executed_or_echoed(self):
        marker = self.root / "injected"
        text = f'Fix $(touch {marker})\n::error::injected\n`touch {marker}`'
        self.data["pr"]["body"] = text
        result = self.merge(payload(commit_message=text), execute=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(marker.exists())
        self.assertNotIn("::error::injected", result.stdout + result.stderr)
        sent = [r for r in self.requests() if r["endpoint"].endswith("/merge")]
        self.assertEqual(json.loads(sent[0]["body"])["commit_message"], text)

    def test_api_failure_is_not_reported_as_clean_or_echoed(self):
        self.data["api_error"] = True
        result = self.pr()
        self.assertEqual(result.returncode, 1)
        self.assertIn("GitHub API read/write failed", result.stderr)
        self.assertNotIn("untrusted API error", result.stderr)

    def test_duplicate_payload_fields_and_malformed_pages_fail_closed(self):
        path = self.root / "payload.json"
        path.write_text('{"sha":"' + HEAD + '","sha":"' + "e" * 40 + '"}')
        result = self.run_cli("merge", "--repo", "example/project", "--pr", "7", "--payload", str(path))
        self.assertEqual(result.returncode, 1)
        self.assertIn("duplicate merge JSON field", result.stderr)
        self.data["pages"][0]["base_commit"] = None
        result = self.pr()
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("Traceback", result.stderr)

    def test_server_added_attribution_is_detected_after_merge(self):
        self.data["merged_message"] = "Merge validated change\n\nCo-authored-by: server-added"
        result = self.merge(execute=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("completed merge message", result.stderr)
        self.assertTrue(any(r["endpoint"].endswith("/merge") for r in self.requests()))


if __name__ == "__main__":
    unittest.main(verbosity=2)
