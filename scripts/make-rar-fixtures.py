# Usage: python scripts/make-rar-fixtures.py <dir>; then set CONVERTHUB_E2E_RAR_DIR=<dir>
# Minimal RAR5 writer (store method only) following the published RAR 5.0
# archive format: https://www.rarlab.com/technote.htm
import struct, sys, zlib, os

def vint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)

def header(htype, hflags, body, data_size=None):
    fields = vint(htype) + vint(hflags)
    if data_size is not None:
        fields += vint(data_size)
    fields += body
    sized = vint(len(fields)) + fields
    return struct.pack("<I", zlib.crc32(sized) & 0xFFFFFFFF) + sized

def rar5(entries):
    out = b"Rar!\x1a\x07\x01\x00"
    out += header(1, 0, vint(0))                      # main archive header
    for name, data in entries:
        body = (vint(0x0004)                           # file flags: CRC32 present
                + vint(len(data)) + vint(0x20)         # unpacked size, attributes
                + struct.pack("<I", zlib.crc32(data) & 0xFFFFFFFF)
                + vint(0) + vint(0)                    # compression: store; host OS: Windows
                + vint(len(name.encode())) + name.encode())
        out += header(2, 0x0002, body, data_size=len(data)) + data
    out += header(5, 0, vint(0))                      # end of archive
    return out

d = sys.argv[1]
open(os.path.join(d, "good.rar"), "wb").write(rar5([("readme.txt", b"hello from rar\n"), ("docs/notes.txt", b"nested file\n")]))
open(os.path.join(d, "traversal.rar"), "wb").write(rar5([("ok.txt", b"fine"), ("../evil.txt", b"escaped!")]))
