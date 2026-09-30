#!/usr/bin/env python3
import argparse
import gzip
import json
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


def host_triple() -> str:
    proc = subprocess.run(
        ["rustc", "-vV"], check=True, stdout=subprocess.PIPE, text=True
    )
    for line in proc.stdout.splitlines():
        if line.startswith("host: "):
            return line.split(": ", 1)[1].strip()
    raise RuntimeError("rustc -vV did not report host triple")


def copy_file(source: pathlib.Path, destination: pathlib.Path, executable: bool = False) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    os.chmod(destination, 0o755 if executable else 0o644)


def add_tar_tree(tar: tarfile.TarFile, root: pathlib.Path, prefix: str) -> None:
    for path in sorted(root.rglob("*"), key=lambda value: value.as_posix()):
        relative = path.relative_to(root).as_posix()
        arcname = f"{prefix}/{relative}" if relative else prefix
        info = tar.gettarinfo(str(path), arcname)
        info.uid = 0
        info.gid = 0
        info.uname = ""
        info.gname = ""
        info.mtime = 0
        if path.is_file():
            with path.open("rb") as handle:
                tar.addfile(info, handle)
        else:
            tar.addfile(info)


def create_tar_gz(stage: pathlib.Path, package_name: str, output: pathlib.Path) -> None:
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode="w") as tar:
                add_tar_tree(tar, stage, package_name)


def create_zip(stage: pathlib.Path, package_name: str, output: pathlib.Path) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*"), key=lambda value: value.as_posix()):
            if not path.is_file():
                continue
            relative = path.relative_to(stage).as_posix()
            info = zipfile.ZipInfo(f"{package_name}/{relative}")
            info.date_time = (1980, 1, 1, 0, 0, 0)
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = 0o755 if path.name in ("codex-tui", "codex-tui.exe") else 0o644
            info.external_attr = mode << 16
            archive.writestr(info, path.read_bytes())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--platform", choices=["linux", "macos", "windows"], required=True)
    parser.add_argument("--notices", required=True)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()

    root = pathlib.Path.cwd()
    binary = pathlib.Path(args.binary)
    notices = pathlib.Path(args.notices)
    if not binary.is_file():
        raise SystemExit(f"binary does not exist: {binary}")
    if not notices.is_file():
        raise SystemExit(f"notices do not exist: {notices}")

    triple = host_triple()
    package_name = f"codex-tui-{args.version}-{triple}"
    output_dir = pathlib.Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory() as temp:
        stage = pathlib.Path(temp) / package_name
        stage.mkdir()
        binary_name = "codex-tui.exe" if args.platform == "windows" else "codex-tui"
        copy_file(binary, stage / binary_name, executable=True)
        copy_file(root / "README.md", stage / "README.md")
        copy_file(root / "CHANGELOG.md", stage / "CHANGELOG.md")
        copy_file(
            root / "docs/release/install-upgrade.md",
            stage / "INSTALL-UPGRADE.md",
        )
        copy_file(notices, stage / "THIRD_PARTY_NOTICES.txt")
        copy_file(
            root / "release/v1.0-criteria.json",
            stage / "V1-STABLE-CRITERIA.json",
        )

        for license_name in ("LICENSE", "LICENSE.txt", "LICENSE.md"):
            license_path = root / license_name
            if license_path.is_file():
                copy_file(license_path, stage / license_name)
                break

        metadata = {
            "schema": "codex-tui/release-artifact/v1",
            "version": args.version,
            "tag": args.tag,
            "commitSha": args.commit,
            "platform": args.platform,
            "hostTriple": triple,
            "binary": binary_name,
        }
        (stage / "RELEASE-METADATA.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )

        if args.platform == "windows":
            output = output_dir / f"{package_name}.zip"
            create_zip(stage, package_name, output)
        else:
            output = output_dir / f"{package_name}.tar.gz"
            create_tar_gz(stage, package_name, output)

    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
