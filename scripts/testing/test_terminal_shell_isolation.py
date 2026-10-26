from pathlib import Path
import subprocess
import tempfile
import unittest

from runtime_fault_regression import isolated_drawer_environment


class DrawerShellIsolation(unittest.TestCase):
    def test_login_argv0_still_enters_only_non_login_fixture_shell(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            for name in (".profile", ".bash_profile", ".bashrc", ".zshrc"):
                (home / name).write_text("echo unsafe > startup-was-read\n")
            env = isolated_drawer_environment(home)
            self.assertEqual(env["ENV"], "")
            self.assertEqual(env["BASH_ENV"], "")
            self.assertNotIn("OPENAI_API_KEY", env)
            result = subprocess.run(["-fixture-shell"], executable=env["SHELL"],
                                    env=env, cwd=home, input="printf ISOLATED\nexit\n",
                                    capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "ISOLATED")
            self.assertFalse((home / "startup-was-read").exists())


if __name__ == "__main__":
    unittest.main()
