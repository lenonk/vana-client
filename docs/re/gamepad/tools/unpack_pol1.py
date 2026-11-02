"""Rebuild FFXiMain.dll with .text decompressed from the POL1 section (static copy for analysis).
Mirrors the entry stub at 0x10be4aab: flag byte MSB-first, 1 = literal, 0 = 2-byte backref
(off = 12 bits, len = hi nibble + 3, off 0 = end)."""
import sys, struct, pefile
src, out = sys.argv[1], sys.argv[2]
pe = pefile.PE(src)
data = bytearray(open(src, 'rb').read())
pol = next(s for s in pe.sections if s.Name.rstrip(b'\0') == b'POL1')
comp = pol.get_data()[:0x1e5a10]
o = bytearray(); i = 0
while True:
    flags = comp[i]; i += 1; done = False
    for _ in range(8):
        lit = flags & 0x80; flags = (flags << 1) & 0xff
        if lit:
            o.append(comp[i]); i += 1
        else:
            b0, b1 = comp[i], comp[i+1]
            off = ((b0 << 8) | b1) & 0xfff
            if off == 0: done = True; break
            i += 2
            n = (b0 >> 4) + 3
            for _ in range(n): o.append(o[-off])
    if done: break
print('decompressed', hex(len(o)), 'expected 0x32762e')
text = next(s for s in pe.sections if s.Name.rstrip(b'\0') == b'.text')
fa = pe.OPTIONAL_HEADER.FileAlignment
pad = (-len(data)) % fa; data += b'\0' * pad
raw_off = len(data)
blob = bytes(o) + b'\0' * ((-len(o)) % fa)
data += blob
# patch .text header: PointerToRawData, SizeOfRawData
hdr = text.get_file_offset()
struct.pack_into('<I', data, hdr + 16, len(blob))
struct.pack_into('<I', data, hdr + 20, raw_off)
# entry -> OEP
struct.pack_into('<I', data, pe.OPTIONAL_HEADER.get_file_offset() + 16, 0x1031676f - 0x10000000)
open(out, 'wb').write(data)
