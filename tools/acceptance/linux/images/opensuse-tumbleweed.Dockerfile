FROM opensuse/tumbleweed:latest
RUN zypper --non-interactive install systemd systemd-resolved dbus-1 gawk iproute2 curl python3 util-linux shadow && zypper clean -a
STOPSIGNAL SIGRTMIN+3
ENTRYPOINT ["/usr/lib/systemd/systemd"]
