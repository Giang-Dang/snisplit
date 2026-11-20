FROM steamos-rootfs:3.8.14
RUN sed -i -E '/by-partsets/ s/^/#/' /etc/fstab && ln -sf /dev/null /etc/systemd/system/etc.mount
STOPSIGNAL SIGRTMIN+3
ENTRYPOINT ["/usr/lib/systemd/systemd"]
