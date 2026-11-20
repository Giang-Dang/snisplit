#!/bin/sh
set -u
export MSYS_NO_PATHCONV=1
here=$(cd "$(dirname "$0")" && { pwd -W 2>/dev/null || pwd; })
binary="$1"
results="$2"
shift 2
distros="${*:-ubuntu-24.04 ubuntu-22.04 debian-12 debian-13 fedora arch opensuse-tumbleweed steamos}"
mkdir -p "$results"
summary="$results/summary.txt"
: > "$summary"

boot_args() {
    case "$1" in
        steamos) echo "systemd.unit=multi-user.target" ;;
        *) echo "" ;;
    esac
}

test_user() {
    case "$1" in
        steamos) echo "deck" ;;
        *) echo "snitest" ;;
    esac
}

for distro in $distros; do
    image="snisplit-m10-$distro"
    container="snisplit-m10-run-$distro"
    log="$results/$distro.log"
    docker rm -f "$container" >/dev/null 2>&1
    if ! docker build -q -t "$image" -f "$here/images/$distro.Dockerfile" "$here/images" > "$log" 2>&1; then
        echo "$distro BUILD-FAILED" | tee -a "$summary"
        continue
    fi
    docker run -d --name "$container" --privileged --cgroupns=host \
        -v /sys/fs/cgroup:/sys/fs/cgroup:rw --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
        "$image" $(boot_args "$distro") >> "$log" 2>&1
    docker exec "$container" sh -c 'timeout 180 systemctl is-system-running --wait' >> "$log" 2>&1
    docker exec "$container" sh -c 'timeout 300 sh -c "until [ -S /run/dbus/system_bus_socket ]; do sleep 1; done"' >> "$log" 2>&1
    if [ "$distro" = steamos ]; then
        docker exec "$container" sh -c 'timeout 300 sh -c "until nmcli general status >/dev/null 2>&1; do sleep 1; done"' >> "$log" 2>&1
    fi
    docker exec "$container" mkdir -p /opt/snisplit
    docker cp "$binary" "$container:/opt/snisplit/snisplit" >> "$log" 2>&1
    for file in prepare.sh check.sh dns_query.py; do
        docker cp "$here/$file" "$container:/opt/snisplit/$file" >> "$log" 2>&1
    done
    docker exec "$container" chmod 755 /opt/snisplit/snisplit /opt/snisplit/prepare.sh /opt/snisplit/check.sh
    user=$(test_user "$distro")
    docker exec "$container" sh -c "id $user >/dev/null 2>&1 || useradd -m $user" >> "$log" 2>&1
    {
        echo "== $distro"
        docker exec "$container" sh -c '. /etc/os-release; echo "os: $PRETTY_NAME"; echo "systemd: $(systemctl --version | head -1)"; echo "kernel: $(uname -r)"'
        echo "== prepare"
        docker exec "$container" sh /opt/snisplit/prepare.sh "$distro"
        echo "prepare-exit=$?"
        echo "== check"
        docker exec -e TEST_USER="$user" "$container" sh /opt/snisplit/check.sh
        echo "check-exit=$?"
    } >> "$log" 2>&1
    passes=$(grep -c '^PASS' "$log")
    fails=$(grep -c '^FAIL' "$log")
    os=$(grep '^os: ' "$log" | head -1 | cut -c5-)
    echo "$distro | $os | pass=$passes fail=$fails" | tee -a "$summary"
    docker rm -f "$container" >/dev/null 2>&1
done
