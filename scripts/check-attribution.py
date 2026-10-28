#!/usr/bin/env python3
"""Check attribution in messages, every PR commit, and explicit merge payloads."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


TRAILERS = re.compile(
    r"^[ \t]*(co-authored-by|authored-by|generated-by|assisted-by|"
    r"ai-assisted-by|ai-generated-by|ai-model|ai-agent|model|agent)[ \t]*:",
    re.IGNORECASE,
)
CREDITS = re.compile(
    r"^[ \t]*(?:🤖[ \t]*)?(?:generated|written|authored|assisted)[ \t]+(?:by|with)\b",
    re.IGNORECASE,
)
# Dependabot's own co-author line on its merged commits. It names a bot
# identity, not AI attribution, so this exact full-line shape is allowed.
BOT_TRAILER = re.compile(
    r"^[ \t]*co-authored-by:[ \t]*dependabot\[bot\] "
    r"<49699333\+dependabot\[bot\]@users\.noreply\.github\.com>[ \t]*$",
    re.IGNORECASE,
)
# Historical commits on develop that predate this rule and cannot be rewritten.
# Dependabot merges are not listed: their bot co-author trailer is allowed by
# BOT_TRAILER, so they need no exemption.
# Each exemption is one exact, immutable commit; a message change produces a
# different SHA and is checked normally. Do not add entries for new work.
HISTORICAL_EXEMPT = {
    "06819e233e76748bd1f9328f852ef75283917a39": "agents-bot squash merge: owner co-author trailer from merge tooling: Add staged Rust formatting hook (#274)",
    "10a64591c70afeb9e527d31fb8a045639d4f59d6": "agents-bot squash merge: owner co-author trailer from merge tooling: Gate terminal browser snapshots and frame times (#310)",
    "4f3b58e46ea25853b8c5de4e48490582955f66f0": "agents-bot squash merge: owner co-author trailer from merge tooling: Add an opt-in saved-report terminal browser (#290)",
    "6106a2d51a7bacaa805a6e3c106ee8c8708c556e": "agents-bot squash merge: owner co-author trailer from merge tooling: Report environment-caused test failures accurately (#315)",
    "83fcc64e3af7be4033899954b538b43b9c987018": "agents-bot squash merge: owner co-author trailer from merge tooling: Flag deleted test assertions as test_value.weakening (#313)",
    "858a515964e4c689c858edc87fcec396360f462d": "agents-bot squash merge: owner co-author trailer from merge tooling: Enable Windows console VT mode and width for pretty output (",
    "9399cea094e68552525a0b1a6d3f7de0d0bd84e3": "agents-bot squash merge: owner co-author trailer from merge tooling: Recognize Rust library target names in SCA (#284)",
    "aefbaff5f6fdecf4b892e33f40863db9046b6226": "agents-bot squash merge: owner co-author trailer from merge tooling: Add stable finding fingerprints (#316)",
    "b000c2463be0399584264fe679eca8e1a2438ea3": "agents-bot squash merge: owner co-author trailer from merge tooling: Summarize fixes in large reports (#322)",
    "cd2058a4b5efe2eccc24b438ea9c4538fb60bbd3": "agents-bot squash merge: owner co-author trailer from merge tooling: Report command timeout details and exhausted analysis budget",
    "f3232fe173f5057f0a3a70831e271e0793de5625": "agents-bot squash merge: owner co-author trailer from merge tooling: Stop Windows command descendants with job objects (#312)",
    "ff58f4ae456f5c141015bde55851ba4b9f8d875c": "agents-bot squash merge: owner co-author trailer from merge tooling: Run independent Rust scans alongside Cargo phases (#339)",
    "4c8202e7ab8db860760bf8628c202fa1dec10eb2": "agents-bot squash merge: owner co-author trailer from merge tooling: Add opt-in multi-pack analysis (#343)",
}
SHA = re.compile(r"[0-9a-f]{40}")
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")


class CheckError(Exception):
    pass


def check_message(text, label, allow_bot_trailer=False):
    for number, line in enumerate(text.splitlines(), 1):
        if allow_bot_trailer and BOT_TRAILER.match(line):
            continue
        if TRAILERS.match(line) or CREDITS.match(line):
            # Do not echo untrusted message text or workflow commands to logs.
            raise CheckError(
                f"{label}, line {number}: prohibited attribution; remove the attribution line"
            )


def api(endpoint, *, paginated=False, payload=None):
    args = ["gh", "api", "--hostname", "github.com", endpoint]
    if paginated:
        args += ["--paginate", "--slurp"]
    if payload is not None:
        args += ["--method", "PUT", "--input", "-"]
    try:
        result = subprocess.run(
            args, input=payload, capture_output=True, text=True, timeout=120, check=True
        )
        return json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        raise CheckError(
            "GitHub API read/write failed; check gh authentication and repository permissions, then retry"
        ) from error


def require_sha(value):
    if not isinstance(value, str) or SHA.fullmatch(value) is None:
        raise CheckError("expected a full lowercase 40-character commit SHA")
    return value


def snapshot(data, expected_head):
    if not isinstance(data, dict):
        raise CheckError("invalid PR response; retry the check")
    try:
        base = require_sha(data["base"]["sha"])
        head = require_sha(data["head"]["sha"])
        count = data["commits"]
        title = data["title"]
        body = data["body"]
        state = data["state"]
    except (KeyError, TypeError) as error:
        raise CheckError("incomplete PR response; retry the check") from error
    if state != "open" or head != expected_head:
        raise CheckError("PR is closed or its head changed; rerun at the current open PR head")
    if type(count) is not int or count < 1 or not isinstance(title, str):
        raise CheckError("invalid PR commit count or title; retry the check")
    if body is not None and not isinstance(body, str):
        raise CheckError("invalid PR body; retry the check")
    return base, head, count, title, body or ""


def check_pr(repository, number, expected_head):
    endpoint = f"repos/{repository}/pulls/{number}"
    before = snapshot(api(endpoint), expected_head)
    base, head, count, title, body = before
    check_message(title, "PR title")
    check_message(body, "PR body")
    # The PR-commits endpoint stops at 250. Paginated comparison has no such cap.
    pages = api(
        f"repos/{repository}/compare/{base}...{head}?per_page=100", paginated=True
    )
    if not isinstance(pages, list) or not pages:
        raise CheckError("missing comparison pages; retry the check")
    seen = set()
    for page in pages:
        if not isinstance(page, dict):
            raise CheckError("invalid comparison page; retry the check")
        page_base = page.get("base_commit")
        if (
            type(page.get("total_commits")) is not int
            or page["total_commits"] != count
            or not isinstance(page_base, dict)
            or page_base.get("sha") != base
        ):
            raise CheckError("comparison does not match the PR snapshot; rerun the check")
        commits = page.get("commits")
        if not isinstance(commits, list):
            raise CheckError("missing comparison commits; retry the check")
        for commit in commits:
            try:
                oid = require_sha(commit["sha"])
                message = commit["commit"]["message"]
            except (KeyError, TypeError) as error:
                raise CheckError("incomplete commit response; retry the check") from error
            if oid in seen or not isinstance(message, str):
                raise CheckError("duplicate commit or missing message; retry the check")
            seen.add(oid)
            if oid not in HISTORICAL_EXEMPT:
                check_message(message, f"commit {oid}", allow_bot_trailer=True)
    if len(seen) != count or head not in seen:
        raise CheckError("incomplete commit pagination; rerun the check")
    if snapshot(api(endpoint), expected_head) != before:
        raise CheckError("PR changed during validation; rerun the check")
    print(f"PASS: {count} PR commit messages and current PR title/body at {head}")


def unique_fields(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise CheckError("duplicate merge JSON field; provide each field exactly once")
        result[key] = value
    return result


def merge_payload(path):
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=unique_fields)
    except (OSError, ValueError) as error:
        raise CheckError("cannot read merge JSON; provide an explicit merge payload file") from error
    keys = {"sha", "merge_method", "commit_title", "commit_message"}
    if not isinstance(data, dict) or set(data) != keys:
        raise CheckError("merge JSON must contain only sha, merge_method, commit_title, commit_message")
    require_sha(data["sha"])
    if data["merge_method"] not in ("merge", "squash"):
        raise CheckError("validate a merge or squash message; rebase has no explicit merge message")
    if not isinstance(data["commit_title"], str) or not data["commit_title"].strip():
        raise CheckError("merge JSON needs a nonempty commit_title")
    if not isinstance(data["commit_message"], str):
        raise CheckError("merge JSON needs an explicit string commit_message, even when empty")
    check_message(data["commit_title"], "proposed merge title")
    check_message(data["commit_message"], "proposed merge body")
    return data


def repository_name(value):
    if REPOSITORY.fullmatch(value) is None or any(part in (".", "..") for part in value.split("/")):
        raise argparse.ArgumentTypeError("use OWNER/REPO, with no URL or options")
    return value


def positive_number(value):
    if not value.isascii() or not value.isdigit() or int(value) < 1:
        raise argparse.ArgumentTypeError("PR number must be a positive integer")
    return int(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    message = commands.add_parser("message", help="check an exact UTF-8 message file")
    message.add_argument("file")
    pr = commands.add_parser("pr", help="check all commits and current PR title/body")
    merge = commands.add_parser("merge", help="validate an explicit proposed merge payload")
    for command in (pr, merge):
        command.add_argument("--repo", type=repository_name, default="moonbase2090/Scorecard")
        command.add_argument("--pr", type=positive_number, required=True)
    pr.add_argument("--head", required=True)
    merge.add_argument("--payload", required=True)
    merge.add_argument("--execute", action="store_true", help="owner only: submit the validated payload")
    args = parser.parse_args()
    try:
        if args.command == "message":
            check_message(Path(args.file).read_text(encoding="utf-8"), "message")
            print("PASS: message has no prohibited attribution")
        elif args.command == "pr":
            check_pr(args.repo, args.pr, require_sha(args.head))
        else:
            payload = merge_payload(args.payload)
            # Serialize once after validation; never ask GitHub to generate default text.
            serialized = json.dumps(payload, ensure_ascii=True)
            digest = hashlib.sha256(serialized.encode("utf-8")).hexdigest()
            if not args.execute:
                print(f"PASS: exact proposed payload sha256={digest}; nothing submitted")
                return 0
            check_pr(args.repo, args.pr, payload["sha"])
            if payload["merge_method"] != "merge":
                raise CheckError("Scorecard permits merge commits only; change merge_method to merge")
            user = api("user")
            if not isinstance(user, dict) or user.get("login") != "mb2090":
                raise CheckError("only the owner account may execute this merge")
            result = api(f"repos/{args.repo}/pulls/{args.pr}/merge", payload=serialized)
            if not isinstance(result, dict) or result.get("merged") is not True:
                raise CheckError("GitHub did not confirm a merge; inspect the PR before retrying")
            merged_sha = require_sha(result.get("sha"))
            actual = api(f"repos/{args.repo}/git/commits/{merged_sha}")
            if not isinstance(actual, dict) or not isinstance(actual.get("message"), str):
                raise CheckError("merge completed but final message could not be verified; inspect the merge")
            check_message(actual["message"], "completed merge message, inspect history without rewriting")
            print(f"MERGED: {merged_sha}; submitted payload sha256={digest}")
    except (CheckError, OSError, UnicodeError) as error:
        print(f"check-attribution: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
