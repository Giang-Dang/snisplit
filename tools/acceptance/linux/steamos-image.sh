#!/bin/sh
set -eu
export MSYS_NO_PATHCONV=1
work="$1"
version="${2:-3.8.14}"
release="${3:-steamdeck-oobe-repair-20260707.10-3.8.14}"
url="https://steamdeck-images.steamos.cloud/recovery/$release.img.bz2"
image="steamdeck-repair-$version.img"
tag="steamos-rootfs:$version"

mkdir -p "$work"
cd "$work"
mount_dir=$(pwd -W 2>/dev/null || pwd)

if [ ! -f "$image.bz2" ]; then
    curl -fSL --retry 5 -C - -o "$image.bz2" "$url"
fi

docker run --rm -v "$mount_dir":/img -w /img ubuntu:24.04 sh -c "
    apt-get update -qq >/dev/null && apt-get install -y -qq lbzip2 fdisk >/dev/null 2>&1
    [ -f '$image' ] || lbunzip2 -k '$image.bz2'
    sfdisk -J '$image' > partitions.json
"

root_start=$(docker run --rm -v "$mount_dir":/img -w /img python:3.12-slim python -I -c "
import json
table = json.load(open('partitions.json'))['partitiontable']
root = [p for p in table['partitions'] if p['type'].upper() == '4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709']
print(root[0]['start'] * table.get('sectorsize', 512))
")

docker run --rm --privileged -v "$mount_dir":/img ubuntu:24.04 sh -c "
    mkdir /r && mount -o ro,loop,offset=$root_start /img/$image /r
    head -3 /r/etc/os-release >&2
    tar -C /r --numeric-owner --xattrs -cf - .
" | docker import - "$tag"

docker images "$tag"
