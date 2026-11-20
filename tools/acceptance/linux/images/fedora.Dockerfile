FROM fedora:latest
RUN dnf install -y systemd systemd-resolved iproute curl python3 util-linux && dnf clean all
STOPSIGNAL SIGRTMIN+3
ENTRYPOINT ["/usr/lib/systemd/systemd"]
