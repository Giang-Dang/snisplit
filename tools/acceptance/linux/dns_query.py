import socket
import struct
import sys

name = sys.argv[1]
with open("/etc/resolv.conf") as conf:
    server = next(line.split()[1] for line in conf if line.startswith("nameserver"))
labels = b"".join(bytes([len(part)]) + part.encode() for part in name.split("."))
query = struct.pack(">HHHHHH", 0x5353, 0x0100, 1, 0, 0, 0) + labels + b"\x00\x00\x01\x00\x01"
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(5)
sock.sendto(query, (server, 53))
reply = sock.recv(4096)
offset = 12 + len(labels) + 1 + 4
answers = struct.unpack(">H", reply[6:8])[0]
addresses = []
for _ in range(answers):
    if reply[offset] & 0xC0 == 0xC0:
        offset += 2
    else:
        while reply[offset] != 0:
            offset += reply[offset] + 1
        offset += 1
    kind, _, _, length = struct.unpack(">HHIH", reply[offset:offset + 10])
    offset += 10
    if kind == 1 and length == 4:
        addresses.append(".".join(str(byte) for byte in reply[offset:offset + 4]))
    offset += length
print(server, " ".join(addresses) if addresses else "none")
