#!/usr/bin/env python3
"""Check freshly built modules against modules the robot already runs.

CONFIG_MODVERSIONS makes insmod compare a CRC per imported symbol, so a
reconstructed kernel that differs from LG's original in any struct layout
produces different CRCs and the module is rejected.  That check only happens
on the device.  This script brings it forward: LG ships its own modules in the
firmware, so every symbol both sides import can be compared offline.

  verify-module-abi.py --reference lg/*.ko -- out/usb-tether-modules/*.ko

Exits non-zero on a vermagic or CRC mismatch.  Symbols that no reference
module imports cannot be checked here and are reported as such -- insmod
remains the final authority, and it fails closed.
"""

import argparse
import struct
import sys

MODVERSION_ENTRY = 64  # struct modversion_info on 32-bit: u32 crc + char[60]


def read_sections(path):
    data = open(path, "rb").read()
    if data[:4] != b"\x7fELF" or data[4] != 1:
        raise ValueError(f"{path}: not a 32-bit ELF object")
    endian = "<" if data[5] == 1 else ">"
    (sh_off,) = struct.unpack_from(endian + "I", data, 0x20)
    sh_entsize, sh_num, sh_strndx = struct.unpack_from(endian + "HHH", data, 0x2E)

    headers = []
    for index in range(sh_num):
        base = sh_off + index * sh_entsize
        name, _type, _flags, _addr, off, size = struct.unpack_from(
            endian + "IIIIII", data, base
        )
        headers.append((name, off, size))

    strtab = headers[sh_strndx][1]
    sections = {}
    for name, off, size in headers:
        end = data.index(b"\0", strtab + name)
        sections[data[strtab + name : end].decode()] = data[off : off + size]
    return sections, endian


def read_module(path):
    sections, endian = read_sections(path)

    vermagic = None
    for field in sections.get(".modinfo", b"").split(b"\0"):
        if field.startswith(b"vermagic="):
            vermagic = field[len(b"vermagic=") :].decode()

    crcs = {}
    versions = sections.get("__versions", b"")
    for offset in range(0, len(versions) - MODVERSION_ENTRY + 1, MODVERSION_ENTRY):
        (crc,) = struct.unpack_from(endian + "I", versions, offset)
        name = versions[offset + 4 : offset + MODVERSION_ENTRY].split(b"\0")[0]
        if name:
            crcs[name.decode("ascii", "replace")] = crc
    return vermagic, crcs


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", nargs="+", required=True,
                        help="modules taken from the robot's own firmware")
    parser.add_argument("candidates", nargs="+",
                        help="modules produced by this build")
    args = parser.parse_args()

    reference_crcs = {}
    reference_vermagic = set()
    for path in args.reference:
        vermagic, crcs = read_module(path)
        if vermagic:
            reference_vermagic.add(vermagic)
        for symbol, crc in crcs.items():
            previous = reference_crcs.setdefault(symbol, crc)
            if previous != crc:
                print(f"reference modules disagree on {symbol}", file=sys.stderr)
                return 2

    print(f"reference: {len(args.reference)} modules, "
          f"{len(reference_crcs)} distinct symbols")
    for vermagic in sorted(reference_vermagic):
        print(f"reference vermagic: {vermagic!r}")

    failed = False
    for path in args.candidates:
        vermagic, crcs = read_module(path)
        name = path.replace("\\", "/").rsplit("/", 1)[-1]

        if vermagic not in reference_vermagic:
            print(f"FAIL {name}: vermagic {vermagic!r} not among reference values")
            failed = True

        checked = sorted(set(crcs) & set(reference_crcs))
        mismatched = [s for s in checked if crcs[s] != reference_crcs[s]]
        unchecked = len(crcs) - len(checked)

        for symbol in mismatched:
            print(f"FAIL {name}: {symbol} crc "
                  f"{crcs[symbol]:#010x} != {reference_crcs[symbol]:#010x}")
            failed = True

        if not mismatched:
            print(f"ok   {name}: {len(checked)}/{len(crcs)} symbols match, "
                  f"{unchecked} not covered by any reference module")

    if failed:
        print("\nABI check failed -- do not load these modules.")
        return 1
    print("\nEvery comparable symbol matches. insmod on the device remains the "
          "final check for the symbols listed as not covered.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
