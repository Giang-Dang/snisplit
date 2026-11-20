#!/bin/sh
set -u
bin=/opt/snisplit/snisplit
query=/opt/snisplit/dns_query.py
out=/root/snisplit-run.out
listed=store.steampowered.com
listed_address=127.0.0.2
second=medium.com
second_address=127.0.0.3
unlisted=example.org
user_site=example.com
failures=0
run_pid=

pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; failures=$((failures + 1)); }
expect() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "expected [$3], got [$2]"; fi; }

links() { ls -d /sys/class/net/snisplit* 2>/dev/null | wc -l | tr -d ' '; }
nss() { getent ahostsv4 "$1" | awk 'NR == 1 { print $1 }'; }
direct() { python3 "$query" "$1" | awk '{ print $2 }'; }
address_kind() { case "$1" in 127.*) echo loopback ;; [0-9]*.[0-9]*.[0-9]*.[0-9]*) echo public ;; *) echo none ;; esac; }

start_run() {
    : > "$out"
    "$bin" run "$@" > "$out" 2>&1 &
    run_pid=$!
    tries=0
    while [ "$tries" -lt 40 ]; do
        if grep -q "Ctrl+C to stop" "$out"; then return 0; fi
        if ! kill -0 "$run_pid" 2>/dev/null; then return 1; fi
        sleep 0.5
        tries=$((tries + 1))
    done
    return 1
}

stopped_within() {
    tries=0
    while kill -0 "$run_pid" 2>/dev/null; do
        [ "$tries" -ge "$1" ] && return 1
        sleep 0.5
        tries=$((tries + 1))
    done
    return 0
}

expect "before:no-links" "$(links)" 0
expect "before:resolv-conf-reaches-stub" "$(awk '/^nameserver/ { print $2; exit }' /etc/resolv.conf)" 127.0.0.53
expect "before:unlisted-nss-not-loopback" "$(address_kind "$(nss $unlisted)")" public
expect "before:unlisted-direct-not-loopback" "$(address_kind "$(direct $unlisted)")" public
expect "before:listed-nss-not-loopback" "$(address_kind "$(nss $listed)")" public

nobody_output=$(setpriv --reuid=65534 --regid=65534 --clear-groups "$bin" run 2>&1)
nobody_status=$?
if [ "$nobody_status" -ne 0 ] && echo "$nobody_output" | grep -qi "sudo"; then pass "non-root:refused-with-sudo-hint"; else fail "non-root:refused-with-sudo-hint" "exit $nobody_status: $nobody_output"; fi
expect "non-root:no-links" "$(links)" 0

if start_run --verbose; then pass "run:ready"; else fail "run:ready" "$(cat "$out")"; fi
expect "run:one-link-per-profile" "$(links)" 2
expect "run:listed-nss" "$(nss $listed)" "$listed_address"
expect "run:listed-subdomain-nss" "$(nss cdn.steampowered.com)" "$listed_address"
expect "run:second-profile-nss" "$(nss $second)" "$second_address"
expect "run:listed-direct" "$(direct $listed)" "$listed_address"
expect "run:unlisted-nss-not-loopback" "$(address_kind "$(nss $unlisted)")" public
expect "run:unlisted-direct-not-loopback" "$(address_kind "$(direct $unlisted)")" public
https=$(curl -s -o /dev/null --max-time 30 -w '%{remote_ip} %{http_code}' "https://$listed/")
expect "run:https-through-relay-ip" "${https% *}" "$listed_address"
case "${https#* }" in 2??|3??) pass "run:https-status ${https#* }" ;; *) fail "run:https-status" "$https" ;; esac
http=$(curl -s -o /dev/null --max-time 30 -w '%{remote_ip} %{http_code}' "http://$listed/")
expect "run:http-through-relay-ip" "${http% *}" "$listed_address"
case "${http#* }" in 2??|3??) pass "run:http-status ${http#* }" ;; *) fail "run:http-status" "$http" ;; esac
if grep -q "$listed" "$out"; then pass "run:relay-logged-listed-name"; else fail "run:relay-logged-listed-name" "$(tail -5 "$out")"; fi
second_output=$("$bin" run 2>&1)
second_status=$?
if [ "$second_status" -ne 0 ] && echo "$second_output" | grep -q "already running"; then pass "run:second-instance-refused"; else fail "run:second-instance-refused" "exit $second_status: $second_output"; fi
expect "run:links-kept-after-second-instance" "$(links)" 2

kill -INT "$run_pid"
if stopped_within 20; then pass "sigint:exited"; else fail "sigint:exited" "still running"; fi
wait "$run_pid"
expect "sigint:exit-code" "$?" 0
expect "sigint:no-links" "$(links)" 0
expect "sigint:listed-nss-not-loopback" "$(address_kind "$(nss $listed)")" public
expect "sigint:listed-direct-not-loopback" "$(address_kind "$(direct $listed)")" public

start_run || fail "kill9:ready" "$(cat "$out")"
kill -KILL "$run_pid"; wait "$run_pid" 2>/dev/null
expect "kill9:links-left" "$(links)" 2
"$bin" cleanup > /root/cleanup.out 2>&1
expect "cleanup:exit-code" "$?" 0
expect "cleanup:no-links" "$(links)" 0
expect "cleanup:listed-nss-not-loopback" "$(address_kind "$(nss $listed)")" public

start_run || fail "stale:first-ready" "$(cat "$out")"
kill -KILL "$run_pid"; wait "$run_pid" 2>/dev/null
if start_run; then pass "stale:next-run-ready"; else fail "stale:next-run-ready" "$(cat "$out")"; fi
expect "stale:one-link-per-profile" "$(links)" 2
expect "stale:listed-nss" "$(nss $listed)" "$listed_address"
kill -TERM "$run_pid"
if stopped_within 20; then pass "sigterm:exited"; else fail "sigterm:exited" "still running"; fi
expect "sigterm:no-links" "$(links)" 0

user_home=$(getent passwd "$TEST_USER" | cut -d: -f6)
user_uid=$(getent passwd "$TEST_USER" | cut -d: -f3)
mkdir -p "$user_home/.config/snisplit/sites"
printf 'name example\ndomains %s\ntls-ports 443\n' "$user_site" > "$user_home/.config/snisplit/sites/example.profile"
chown -R "$TEST_USER" "$user_home/.config"
if SUDO_UID="$user_uid" HOME=/root start_run; then pass "sudo-user-sites:ready"; else fail "sudo-user-sites:ready" "$(cat "$out")"; fi
expect "sudo-user-sites:user-site-loopback" "$(address_kind "$(nss $user_site)")" loopback
expect "sudo-user-sites:three-links" "$(links)" 3
kill -INT "$run_pid"; stopped_within 20
expect "sudo-user-sites:no-links" "$(links)" 0
rm -rf "$user_home/.config/snisplit"

echo "SUMMARY failures=$failures"
exit "$failures"
