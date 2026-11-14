import socket
import ssl
import sys

host = sys.argv[1]
target = sys.argv[2]
rounds = int(sys.argv[3])

for r in range(1, rounds + 1):
    ctx = ssl.create_default_context()
    try:
        with socket.create_connection((target, 443), timeout=8) as raw:
            with ctx.wrap_socket(raw, server_hostname=host) as tls:
                tls.sendall(f"HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n".encode())
                line = tls.recv(200).split(b"\r\n")[0].decode(errors="replace")
                print(f"r{r} PASS {tls.version()} group={getattr(tls, 'group', lambda: '?')()} status={line!r}")
    except Exception as e:
        print(f"r{r} FAIL {type(e).__name__}: {e}")
