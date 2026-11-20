FROM cgpanel-mail-test
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    openssh-server libnginx-mod-http-modsecurity vsftpd bubblewrap e2fsprogs util-linux \
    && rm -rf /var/lib/apt/lists/*
