"""Copia accanto a step2wrl.exe le DLL di OpenCASCADE e di terze parti di cui ha bisogno.

Legge la tabella delle importazioni dei file PE e segue le dipendenze a catena, cercando le DLL
nella cartella bin di OpenCASCADE e nelle cartelle bin delle librerie di terze parti.
Uso: python copy_dlls.py <cartella OCCT> <cartella di step2wrl.exe>
"""
import os
import shutil
import struct
import sys


def imports(path):
    with open(path, "rb") as f:
        d = f.read()
    pe = struct.unpack_from("<I", d, 0x3C)[0]
    nsec = struct.unpack_from("<H", d, pe + 6)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", d, opt)[0]
    dirs = opt + (112 if magic == 0x20B else 96)
    rva = struct.unpack_from("<I", d, dirs + 8)[0]
    secs = opt + struct.unpack_from("<H", d, pe + 20)[0]

    def off(r):
        for i in range(nsec):
            s = secs + 40 * i
            vsize, va, rsize, raw = struct.unpack_from("<IIII", d, s + 8)
            if va <= r < va + max(vsize, rsize):
                return r - va + raw
        return None

    out = []
    p = off(rva)
    while p is not None:
        name_rva = struct.unpack_from("<I", d, p + 12)[0]
        if name_rva == 0:
            break
        q = off(name_rva)
        out.append(d[q:d.index(b"\0", q)].decode())
        p += 20
    return out


def main():
    occt, dest = sys.argv[1], sys.argv[2]
    search = [os.path.join(occt, "win64", "vc14", "bin")]
    third = os.path.join(os.path.dirname(occt), "3rdparty-vc14-64")
    if os.path.isdir(third):
        for name in os.listdir(third):
            for sub in ("bin", os.path.join("bin", "win64")):
                b = os.path.join(third, name, sub)
                if os.path.isdir(b):
                    search.append(b)
    index = {}
    for s in search:
        for f in os.listdir(s):
            if f.lower().endswith(".dll"):
                index.setdefault(f.lower(), os.path.join(s, f))
    todo, done = [os.path.join(dest, "step2wrl.exe")], set()
    while todo:
        for dll in imports(todo.pop()):
            k = dll.lower()
            if k in done or k not in index:
                continue  # DLL di sistema o del runtime di Visual C++
            done.add(k)
            shutil.copy2(index[k], os.path.join(dest, os.path.basename(index[k])))
            todo.append(index[k])
    total = sum(os.path.getsize(os.path.join(dest, f)) for f in os.listdir(dest))
    print(f"{len(done)} DLL copiate: {', '.join(sorted(done))}")
    print(f"Totale cartella: {total / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
