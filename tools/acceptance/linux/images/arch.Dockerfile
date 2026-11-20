FROM archlinux:latest
RUN pacman -Syu --noconfirm systemd iproute2 curl python util-linux && pacman -Scc --noconfirm
STOPSIGNAL SIGRTMIN+3
ENTRYPOINT ["/usr/lib/systemd/systemd"]
