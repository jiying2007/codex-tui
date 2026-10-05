import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile
import verify_archive as v

class ArchiveLayout(unittest.TestCase):
    def archive(self, names, suffix=".zip"):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory, "fixture" + suffix)
            destination = Path(directory, "out")
            destination.mkdir()
            if suffix == ".zip":
                with zipfile.ZipFile(archive, "w") as handle:
                    for name in names: handle.writestr(name, b"fixture")
            else:
                with tarfile.open(archive, "w:gz") as handle:
                    for name in names:
                        info = tarfile.TarInfo(name)
                        info.size = 7
                        handle.addfile(info, io.BytesIO(b"fixture"))
            return v.extract(archive, destination).name
    def test_valid_native_layout(self):
        for suffix in (".zip", ".tar.gz"):
            self.assertEqual(self.archive(["root/codex-tui", "root/README.md"], suffix), "root")
    def test_normalized_aliases_and_collisions_are_rejected(self):
        cases = (["root/file", "root/../file"], ["root/file", "root/file"],
                 ["root/file", "root/FILE"], ["root/file", "root\\else"], ["root/file", "loose"])
        for suffix in (".zip", ".tar.gz"):
            for names in cases:
                with self.subTest(suffix=suffix, names=names):
                    with self.assertRaises(SystemExit): self.archive(names, suffix)
