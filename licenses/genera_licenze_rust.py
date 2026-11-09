"""Raccoglie licenze e note di copyright delle librerie Rust incluse in dima3d.exe.

Legge `cargo metadata` (solo le dipendenze che finiscono nell'exe per Windows: niente quelle di
compilazione e dei test) e copia i file di licenza di ogni libreria (LICENSE*, COPYING*, NOTICE*,
COPYRIGHT*). I testi identici (es. Apache 2.0) si scrivono una volta sola.
Uso, dalla cartella del progetto:  python licenses/genera_licenze_rust.py
Risultato: licenses/librerie-rust.txt
"""
import json
import os
import re
import subprocess

# Indirizzi email (con o senza < >): omessi, restano i nomi dei titolari dei diritti
EMAIL = re.compile(r"\s*<?[\w.+-]+@[\w-]+(\.[\w-]+)+>?")
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
USE OR OTHER DEALINGS IN THE SOFTWARE."""
PREFIXES = ("license", "licence", "copying", "notice", "copyright", "unlicense")


def main():
    meta = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"],
            cwd=ROOT,
        )
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]

    # Dipendenze raggiungibili con legami normali (non build, non dev)
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        for dep in nodes[current]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])

    crates = sorted((packages[i] for i in seen), key=lambda p: (p["name"].lower(), p["version"]))
    texts = {}  # testo -> libreria in cui compare per la prima volta
    out = [
        "Dima 3D — librerie Rust incluse nell'eseguibile",
        "=" * 48,
        "",
        f"{len(crates)} librerie. Per ognuna: nome, versione, licenza, sito e i file di licenza e di copyright",
        "distribuiti con la libreria. Un testo identico a uno già riportato è indicato con un rimando.",
        "Gli indirizzi email degli autori sono omessi; nomi e testi delle licenze sono quelli originali.",
        "",
    ]
    for p in crates:
        out.append("-" * 100)
        out.append(f"{p['name']} {p['version']}")
        out.append(f"Licenza: {p.get('license') or '(vedi file)'}")
        if p.get("authors"):
            out.append("Autori: " + ", ".join(a.split(" <")[0] for a in p["authors"]))
        if p.get("repository"):
            out.append(f"Sito: {p['repository']}")
        folder = os.path.dirname(p["manifest_path"])
        files = sorted(
            f for f in os.listdir(folder)
            if f.lower().startswith(PREFIXES) and os.path.isfile(os.path.join(folder, f))
        )
        if p.get("license_file") and os.path.basename(p["license_file"]) not in files:
            files.append(p["license_file"])
        if not files:
            who = ", ".join(a.split(" <")[0] for a in p.get("authors") or []) or f"gli autori di {p['name']}"
            out.append("")
            out.append(f"Copyright (c) {who}")
            out.append("Il pacchetto non contiene file di licenza: vale la licenza indicata sopra, con i testi")
            out.append("standard riportati in fondo a questo file.")
        for f in files:
            with open(os.path.join(folder, f), encoding="utf-8", errors="replace") as fh:
                text = fh.read().strip()
            key = " ".join(text.split())
            out.append("")
            if key in texts:
                out.append(f"[{f}: testo identico a quello di {texts[key]}]")
            else:
                texts[key] = f"{p['name']} {p['version']} ({f})"
                out.append(f"[{f}]")
                out.append(text)
        out.append("")
    out += ["=" * 100, "Testo standard della licenza MIT", "", MIT, "", "=" * 100,
            "Licenza Apache 2.0: il testo completo è riportato sopra (cercare «Apache License»).", ""]
    with open(os.path.join(HERE, "librerie-rust.txt"), "w", encoding="utf-8", newline="\r\n") as fh:
        fh.write(EMAIL.sub("", "\n".join(out) + "\n"))
    print(f"{len(crates)} librerie, {len(texts)} testi diversi")


if __name__ == "__main__":
    main()
