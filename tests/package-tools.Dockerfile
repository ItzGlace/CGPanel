FROM cgpanel-upgrade-test
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
 curl gnupg python3-venv podman uidmap slirp4netns fuse-overlayfs dbus-user-session \
 bind9 nftables fail2ban openssh-server rclone iptables \
 && rm -rf /var/lib/apt/lists/*
