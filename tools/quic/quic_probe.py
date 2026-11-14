import argparse
import asyncio
import os
import socket
import ssl
import sys
import time
from datetime import datetime, timezone

import dns.resolver
from aioquic.asyncio.client import connect
from aioquic.asyncio.protocol import QuicConnectionProtocol
from aioquic.h3.connection import H3_ALPN, H3Connection
from aioquic.h3.events import HeadersReceived
from aioquic.quic.configuration import QuicConfiguration
from aioquic.quic.events import ConnectionTerminated

QUIC_PORT = 443
INITIAL_MIN_SIZE = 1200
UNKNOWN_QUIC_VERSION = bytes.fromhex("1a2a3a4a")
LONG_HEADER_INITIAL_FIRST_BYTE = 0xC0
VERSION_NEGOTIATION_VERSION = bytes(4)


class CountingTransport:
    def __init__(self, inner, counters):
        self._inner = inner
        self._counters = counters

    def sendto(self, data, addr=None):
        self._counters["sent"] += 1
        self._inner.sendto(data, addr)

    def __getattr__(self, name):
        return getattr(self._inner, name)


class ProbeProtocol(QuicConnectionProtocol):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.counters = {"sent": 0, "received": 0}
        self.h3 = None
        self.status_waiter = None
        self.event_names = []
        self.close_reason = ""

    def connection_made(self, transport):
        super().connection_made(CountingTransport(transport, self.counters))

    def datagram_received(self, data, addr):
        self.counters["received"] += 1
        super().datagram_received(data, addr)

    def quic_event_received(self, event):
        self.event_names.append(type(event).__name__)
        if isinstance(event, ConnectionTerminated):
            self.close_reason = f"close code={event.error_code:#x} reason={event.reason_phrase!r}"
        if self.h3 is None:
            return
        for h3_event in self.h3.handle_event(event):
            self.event_names.append(type(h3_event).__name__)
            if isinstance(h3_event, HeadersReceived) and self.status_waiter and not self.status_waiter.done():
                status = dict(h3_event.headers).get(b":status", b"?").decode()
                self.status_waiter.set_result(status)

    async def get_request(self, authority):
        self.h3 = H3Connection(self._quic)
        self.status_waiter = asyncio.get_running_loop().create_future()
        stream_id = self._quic.get_next_available_stream_id()
        self.h3.send_headers(
            stream_id,
            [(b":method", b"GET"), (b":scheme", b"https"), (b":authority", authority.encode()), (b":path", b"/")],
            end_stream=True,
        )
        self.transmit()
        return await self.status_waiter


async def probe_handshake(address, server_name, timeout):
    configuration = QuicConfiguration(is_client=True, alpn_protocols=H3_ALPN, server_name=server_name)
    configuration.verify_mode = ssl.CERT_REQUIRED
    holder = {}
    outcome = {"handshake": "none", "h3_status": "-", "error": ""}

    def make_protocol(*args, **kwargs):
        protocol = ProbeProtocol(*args, **kwargs)
        holder["protocol"] = protocol
        return protocol

    try:
        async with asyncio.timeout(timeout):
            async with connect(address, QUIC_PORT, configuration=configuration, create_protocol=make_protocol) as client:
                outcome["handshake"] = "complete"
                outcome["h3_status"] = await client.get_request(server_name)
    except TimeoutError:
        outcome["error"] = "timeout"
    except Exception as failure:
        outcome["error"] = f"{type(failure).__name__}: {failure}"
    protocol = holder.get("protocol")
    if protocol is not None:
        outcome["events"] = summarize(protocol.event_names)
        outcome["error"] = " ".join(part for part in (outcome["error"], protocol.close_reason) if part)
        return protocol.counters, outcome
    outcome["events"] = "-"
    return {"sent": 0, "received": 0}, outcome


def summarize(names):
    counts = {}
    for name in names:
        counts[name] = counts.get(name, 0) + 1
    return ",".join(f"{name}x{count}" for name, count in counts.items())


def version_negotiation_probe(address, timeout):
    destination_cid = os.urandom(8)
    source_cid = os.urandom(8)
    header = (
        bytes([LONG_HEADER_INITIAL_FIRST_BYTE])
        + UNKNOWN_QUIC_VERSION
        + bytes([len(destination_cid)]) + destination_cid
        + bytes([len(source_cid)]) + source_cid
    )
    packet = header + bytes(INITIAL_MIN_SIZE - len(header))
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
        udp.settimeout(timeout)
        udp.connect((address, QUIC_PORT))
        udp.send(packet)
        try:
            reply = udp.recv(2048)
        except TimeoutError:
            return "no reply (timeout)"
        except OSError as failure:
            return f"error: {failure}"
    is_version_negotiation = reply[1:5] == VERSION_NEGOTIATION_VERSION
    return f"reply {len(reply)} bytes, version negotiation={is_version_negotiation}"


def now_utc():
    return datetime.now(timezone.utc).strftime("%H:%M:%S")


def run_cases(cases, rounds, timeout):
    for round_number in range(1, rounds + 1):
        for address, server_name in cases:
            started = time.monotonic()
            counters, outcome = asyncio.run(probe_handshake(address, server_name, timeout))
            elapsed_ms = int((time.monotonic() - started) * 1000)
            print(
                f"{now_utc()} round={round_number} addr={address} sni={server_name} "
                f"sent={counters['sent']} received={counters['received']} "
                f"handshake={outcome['handshake']} h3={outcome['h3_status']} ms={elapsed_ms} {outcome['error']} events={outcome['events']}",
                flush=True,
            )


def print_https_records(names):
    resolver = dns.resolver.Resolver()
    for name in names:
        try:
            answer = resolver.resolve(name, "HTTPS")
            for record in answer:
                print(f"{name} HTTPS {record.to_text()}")
        except Exception as failure:
            print(f"{name} HTTPS none ({type(failure).__name__})")


def parse_case(text):
    address, server_name = text.split("=", 1)
    return address, server_name


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    handshake = commands.add_parser("handshake")
    handshake.add_argument("cases", nargs="+", type=parse_case, metavar="ADDRESS=SNI")
    handshake.add_argument("--rounds", type=int, default=3)
    handshake.add_argument("--timeout", type=float, default=8.0)
    liveness = commands.add_parser("version-negotiation")
    liveness.add_argument("addresses", nargs="+")
    liveness.add_argument("--timeout", type=float, default=5.0)
    records = commands.add_parser("https-records")
    records.add_argument("names", nargs="+")
    arguments = parser.parse_args()
    if arguments.command == "handshake":
        run_cases(arguments.cases, arguments.rounds, arguments.timeout)
    elif arguments.command == "version-negotiation":
        for address in arguments.addresses:
            print(f"{now_utc()} addr={address} {version_negotiation_probe(address, arguments.timeout)}", flush=True)
    else:
        print_https_records(arguments.names)
    return 0


if __name__ == "__main__":
    sys.exit(main())
