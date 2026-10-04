import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

try:
    from credential_scan import (  # noqa: E402
        is_sensitive_name,
        scan_file,
        scan_workspace,
    )
except ModuleNotFoundError:
    is_sensitive_name = scan_file = scan_workspace = None


@unittest.skipIf(is_sensitive_name is None, "credential_scan tool unavailable")
class CredentialScanTests(unittest.TestCase):
    def test_sensitive_filename_patterns(self):
        self.assertTrue(is_sensitive_name(Path("TESTACCOUNT.txt")))
        self.assertTrue(is_sensitive_name(Path("GithubToken")))
        self.assertTrue(is_sensitive_name(Path("kindle_key")))
        self.assertFalse(is_sensitive_name(Path("README.md")))

    def test_scan_file_detects_token_content(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bin" / "config.json"
            path.parent.mkdir()
            path.write_text(
                '{"account_password": "not-a-real-password"}',
                encoding="utf-8",
            )
            self.assertEqual(scan_file(path)[0][1], "non-empty account_password")

    def test_scan_workspace_finds_sensitive_file_outside_repo(self):
        with tempfile.TemporaryDirectory() as tmp:
            workspace = Path(tmp) / "workspace"
            repo = workspace / "repo"
            repo.mkdir(parents=True)
            secret = workspace / "TESTACCOUNT.txt"
            secret.write_text("redacted", encoding="utf-8")
            findings = scan_workspace(workspace, repo)
            self.assertEqual(findings[0][0], secret)


if __name__ == "__main__":
    unittest.main()
