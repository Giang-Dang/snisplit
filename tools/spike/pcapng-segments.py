import struct
import sys
from collections import OrderedDict

data = open(sys.argv[1], "rb").read()
targets = set(sys.argv[2].split(","))
pos = 0
linktypes = []
flows = OrderedDict()

while pos + 12 <= len(data):
    btype, blen = struct.unpack_from("<II", data, pos)
    if blen < 12:
        break
    body = data[pos + 8 : pos + blen - 4]
    if btype == 1:
        linktypes.append(struct.unpack_from("<H", body, 0)[0])
    elif btype == 6:
        ifid, th, tl, caplen, origlen = struct.unpack_from("<IIIII", body, 0)
        pkt = body[20 : 20 + caplen]
        lt = linktypes[ifid] if ifid < len(linktypes) else 1
        off = 14 if lt == 1 else 0
        if lt == 1 and len(pkt) >= 14 and struct.unpack_from("!H", pkt, 12)[0] != 0x0800:
            pos += blen
            continue
        ip = pkt[off:]
        if len(ip) >= 20 and ip[0] >> 4 == 4 and ip[9] == 6:
            ihl = (ip[0] & 15) * 4
            total = struct.unpack_from("!H", ip, 2)[0]
            src = ".".join(map(str, ip[12:16]))
            dst = ".".join(map(str, ip[16:20]))
            tcp = ip[ihl:]
            if len(tcp) >= 20:
                sp, dp, seq = struct.unpack_from("!HHI", tcp, 0)
                doff = (tcp[12] >> 4) * 4
                flags = tcp[13]
                plen = total - ihl - doff
                ts = (th << 32 | tl) / 1e6
                if dst in targets and dp in (80, 443):
                    key = (src, sp, dst, dp)
                    f = flows.setdefault(key, {"segs": OrderedDict(), "rst_in": False, "t0": ts})
                    if plen > 0 and len(f["segs"]) < 6:
                        f["segs"].setdefault(seq, plen)
                elif src in targets and sp in (80, 443):
                    key = (dst, dp, src, sp)
                    if key in flows and flags & 4:
                        flows[key]["rst_in"] = True
    pos += blen

for (src, sp, dst, dp), f in flows.items():
    if not f["segs"]:
        continue
    sizes = "+".join(str(v) for v in f["segs"].values())
    print(f"{dst}:{dp} from :{sp}  first payload segments {sizes}  rst_from_server={f['rst_in']}")
