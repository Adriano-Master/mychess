#!/usr/bin/env python3
"""Packs a staged directory into a .tar.gz (Linux) or .zip (Windows).

Usage: pack.py tar|zip STAGE_DIR OUTPUT

STAGE_DIR contains one root folder (MyChess-<version>-<os>/). The archive is
reproducible: sorted entries, fixed mtime (SOURCE_DATE_EPOCH or 2026-01-01),
explicit modes (executables 0755, the rest 0644). The zip stores text files
(.txt, .md, LICENSE, portable.flag aside) with CRLF line ends for Windows.
"""
import gzip
import io
import os
import sys
import tarfile
import zipfile

EPOCH = int(os.environ.get("SOURCE_DATE_EPOCH", "1767225600"))  # 2026-01-01
TEXT_EXT = {".txt", ".md"}
EXEC_NAMES = {"mychess", "stockfish_10_x64_modern_official"}


def entries(root):
    out = []
    for base, dirs, files in os.walk(root):
        dirs.sort()
        rel = os.path.relpath(base, root)
        if rel != ".":
            out.append((rel, True))
        for name in sorted(files):
            out.append((os.path.normpath(os.path.join(rel, name)), False))
    return sorted(out)


def mode_of(path, is_dir):
    if is_dir:
        return 0o755
    return 0o755 if os.path.basename(path) in EXEC_NAMES else 0o644


def is_text(path):
    name = os.path.basename(path)
    return os.path.splitext(name)[1].lower() in TEXT_EXT or name == "LICENSE"


def main():
    kind, stage, output = sys.argv[1:4]
    items = entries(stage)
    if kind == "tar":
        with open(output, "wb") as raw, gzip.GzipFile(
            filename="", mode="wb", fileobj=raw, mtime=EPOCH, compresslevel=9
        ) as gz, tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tar:
            for rel, is_dir in items:
                info = tarfile.TarInfo(rel.replace(os.sep, "/"))
                info.mtime = EPOCH
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mode = mode_of(rel, is_dir)
                full = os.path.join(stage, rel)
                if is_dir:
                    info.type = tarfile.DIRTYPE
                    tar.addfile(info)
                else:
                    info.size = os.path.getsize(full)
                    with open(full, "rb") as fh:
                        tar.addfile(info, fh)
    elif kind == "zip":
        date = tuple(__import__("time").gmtime(EPOCH))[:6]
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            for rel, is_dir in items:
                name = rel.replace(os.sep, "/") + ("/" if is_dir else "")
                info = zipfile.ZipInfo(name, date)
                info.create_system = 3
                info.external_attr = (mode_of(rel, is_dir) | (0o40000 if is_dir else 0o100000)) << 16
                if is_dir:
                    z.writestr(info, b"")
                    continue
                with open(os.path.join(stage, rel), "rb") as fh:
                    data = fh.read()
                if is_text(rel):
                    data = data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, data, compresslevel=9)
    else:
        sys.exit("usage: pack.py tar|zip STAGE_DIR OUTPUT")


if __name__ == "__main__":
    main()
