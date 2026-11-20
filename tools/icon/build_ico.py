import io
import struct
import subprocess
import sys
from pathlib import Path

from PIL import Image

ICON_FOLDER = Path(__file__).resolve().parents[2] / "assets" / "icon"
SMALL_SOURCE = ICON_FOLDER / "snisplit-small.svg"
LARGE_SOURCE = ICON_FOLDER / "snisplit.svg"
OUTPUT = ICON_FOLDER / "snisplit.ico"
WINDOW_ICON = ICON_FOLDER / "snisplit-32.rgba"
WINDOW_ICON_SIZE = 32
SMALL_SIZES = (16, 20, 24, 32)
LARGE_SIZES = (40, 48, 64, 96, 128, 256)
ICONDIR_SIZE = 6
ICONDIRENTRY_SIZE = 16
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def render(rasterizer: str, source: Path, size: int) -> bytes:
    png = subprocess.run(
        [rasterizer, "-w", str(size), "-h", str(size), str(source)],
        check=True,
        capture_output=True,
    ).stdout
    if not png.startswith(PNG_SIGNATURE):
        raise SystemExit(f"{rasterizer} did not return a PNG for {source.name} at {size} px")
    return png


def directory_entry(size: int, length: int, offset: int) -> bytes:
    side_byte = 0 if size == 256 else size
    return struct.pack("<BBBBHHII", side_byte, side_byte, 0, 0, 1, 32, length, offset)


def icon_file(images: list[tuple[int, bytes]]) -> bytes:
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = ICONDIR_SIZE + ICONDIRENTRY_SIZE * len(images)
    entries = b""
    for size, png in images:
        entries += directory_entry(size, len(png), offset)
        offset += len(png)
    return header + entries + b"".join(png for _, png in images)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: python build_ico.py <path to rsvg-convert>")
    rasterizer = sys.argv[1]
    images = [(size, render(rasterizer, SMALL_SOURCE, size)) for size in SMALL_SIZES]
    images += [(size, render(rasterizer, LARGE_SOURCE, size)) for size in LARGE_SIZES]
    OUTPUT.write_bytes(icon_file(images))
    window_png = dict(images)[WINDOW_ICON_SIZE]
    WINDOW_ICON.write_bytes(Image.open(io.BytesIO(window_png)).convert("RGBA").tobytes())
    print(f"{WINDOW_ICON}: {WINDOW_ICON.stat().st_size} bytes")
    print(f"{OUTPUT}: {len(images)} images, {OUTPUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
