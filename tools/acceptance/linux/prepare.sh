#!/bin/sh
set -u
distro="$1"
bin=/opt/snisplit/snisplit
stub=../run/systemd/resolve/stub-resolv.conf
failures=0

pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; failures=$((failures + 1)); }
step() {
    if env "$@" > /root/step.out 2>&1; then pass "enable-step $*"; else fail "enable-step $*" "$(tail -5 /root/step.out)"; fi
}

docker_upstream=$(awk '/^nameserver/ { print $2; exit }' /etc/resolv.conf)
umount /etc/resolv.conf 2>/dev/null
rm -f /etc/resolv.conf
printf '# Resolver configuration file.\n' > /etc/resolv.conf

use_docker_upstream() {
    mkdir -p /etc/systemd/resolved.conf.d
    printf '[Resolve]\nDNS=%s\n' "$docker_upstream" > /etc/systemd/resolved.conf.d/dhcp-stand-in.conf
}

refuses_without_resolved() {
    output=$("$bin" run 2>&1)
    status=$?
    if [ "$status" -ne 0 ] && echo "$output" | grep -q "systemd-resolved"; then
        pass "refuses-without-resolved"
    else
        fail "refuses-without-resolved" "exit $status: $output"
    fi
    if [ -z "$(ls -d /sys/class/net/snisplit* 2>/dev/null)" ]; then
        pass "refusal-left-no-link"
    else
        fail "refusal-left-no-link" "$(ls /sys/class/net)"
    fi
}

as_installed() {
    use_docker_upstream
    ln -sf "$stub" /etc/resolv.conf
    systemctl enable --now systemd-resolved >/dev/null 2>&1
    systemctl restart systemd-resolved
}

case "$distro" in
    ubuntu-24.04|ubuntu-22.04|fedora)
        as_installed
        ;;
    debian-12|debian-13)
        printf 'nameserver %s\n' "$docker_upstream" > /etc/resolv.conf
        refuses_without_resolved
        use_docker_upstream
        step DEBIAN_FRONTEND=noninteractive apt-get install -y systemd-resolved
        systemctl restart systemd-resolved
        ;;
    arch)
        printf 'nameserver %s\n' "$docker_upstream" > /etc/resolv.conf
        refuses_without_resolved
        use_docker_upstream
        step systemctl enable --now systemd-resolved
        ln -sf "$stub" /etc/resolv.conf
        systemctl restart systemd-resolved
        ;;
    opensuse-tumbleweed)
        printf 'nameserver %s\n' "$docker_upstream" > /etc/resolv.conf
        refuses_without_resolved
        use_docker_upstream
        step rpm -q systemd-resolved
        step systemctl enable --now systemd-resolved
        ln -sf "$stub" /etc/resolv.conf
        systemctl restart systemd-resolved
        ;;
    steamos)
        address=$(ip -4 -o addr show eth0 | awk '{ print $4 }')
        gateway=$(ip route | awk '/default/ { print $3 }')
        nmcli con add type ethernet ifname eth0 con-name wifi-stand-in ipv4.method manual ipv4.addresses "$address" ipv4.gateway "$gateway" ipv4.dns "$docker_upstream" ipv6.method disabled >/dev/null
        nmcli con up wifi-stand-in >/dev/null
        tries=0
        until grep -q '^nameserver' /etc/resolv.conf || [ "$tries" -ge 20 ]; do sleep 1; tries=$((tries + 1)); done
        ;;
    *)
        fail "distro" "unknown $distro"
        ;;
esac

if resolvectl status >/dev/null 2>&1; then pass "resolved-answers"; else fail "resolved-answers" "resolvectl status failed"; fi
exit "$failures"
