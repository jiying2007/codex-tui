import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile
import verify_archive as v


class ArchiveLayout(unittest.TestCase):
    def archive(self, names, suffix=".zip", windows_names=False):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory, "fixture" + suffix)
            destination = Path(directory, "out")
            destination.mkdir()
            if suffix == ".zip":
                with zipfile.ZipFile(archive, "w") as handle:
                    for name in names:
                        # ZipInfo construction normalizes backslashes on Windows and
                        # truncates NUL. Assign afterwards to retain malformed wire names.
                        info = zipfile.ZipInfo()
                        info.filename = name
                        info.orig_filename = name
                        handle.writestr(info, b"fixture")
                with zipfile.ZipFile(archive) as handle:
                    self.assertEqual([entry.orig_filename for entry in handle.infolist()], names)
            else:
                with tarfile.open(archive, "w:gz") as handle:
                    for name in names:
                        info = tarfile.TarInfo(name)
                        info.size = 7
                        handle.addfile(info, io.BytesIO(b"fixture"))
            if windows_names:
                # Exercise the reader's Windows normalization on every CI platform.
                with mock.patch("zipfile.os.sep", "\\"):
                    return v.extract(archive, destination).name
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
                    with self.assertRaises(SystemExit):
                        self.archive(names, suffix)

    def test_raw_zip_names_are_not_hidden_by_windows_normalization(self):
        for name in ("root\\else", "root/else\x00hidden"):
            with self.subTest(name=name):
                with self.assertRaises(SystemExit):
                    self.archive(["root/file", name], windows_names=True)
