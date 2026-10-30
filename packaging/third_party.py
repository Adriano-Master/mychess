#!/usr/bin/env python3
"""Writes LICENSES-THIRD-PARTY.txt: name, version, SPDX license and repository
of every crate that is linked into the `mychess` binary for a target.

Usage: third_party.py TARGET_TRIPLE OUTPUT
Uses `cargo tree` (normal dependencies only: build scripts and dev
dependencies do not enter the binary; proc-macros are skipped, they run at
build time). Needs no new dependency.
"""
import re
import subprocess
import sys

HEADER = """Licencas de terceiros do MyChess 0.1.0-alpha
=============================================

O MyChess e distribuido sob a GPL-3.0-or-later (arquivo LICENSE). Abaixo estao as
bibliotecas Rust que entram no executavel desta plataforma ({target}), com a
expressao de licenca SPDX declarada por cada uma (tal como publicada no crates.io;
quando ha "OR", vale a escolha do usuario da biblioteca; o MyChess usa uma opcao
compativel com a GPL-3.0-or-later). Gerado por packaging/third_party.py.
Pecas, fontes, icones e sons embutidos: veja LICENSES-ASSETS.md e CREDITS.md.

nome versao | licenca | repositorio
-------------------------------------
"""


def main():
    target, output = sys.argv[1:3]
    out = subprocess.run(
        ["cargo", "tree", "-p", "mychess-gui", "-e", "normal", "--target", target,
         "--prefix", "none", "--locked", "-f", "{p}|{l}|{r}"],
        check=True, capture_output=True, text=True,
    ).stdout
    rows = {}
    for line in out.splitlines():
        line = line.strip()
        if not line or "(proc-macro)" in line.split("|")[0]:
            continue
        pkg, lic, repo = (line.split("|") + ["", ""])[:3]
        pkg = re.sub(r" \((\*|/.*)\)$", "", pkg).strip()
        pkg = re.sub(r" \(\*\)$", "", pkg)
        m = re.match(r"^(\S+) v(\S+)", pkg)
        if not m or m.group(1).startswith("mychess-"):
            continue
        rows[(m.group(1), m.group(2))] = (lic.strip() or "(nao declarada)", repo.strip())
    with open(output, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(HEADER.format(target=target))
        for (name, ver), (lic, repo) in sorted(rows.items()):
            fh.write(f"{name} {ver} | {lic} | {repo}\n")
        fh.write(f"\n{len(rows)} bibliotecas.\n")
    print(f"{len(rows)} crates -> {output}")


if __name__ == "__main__":
    main()
