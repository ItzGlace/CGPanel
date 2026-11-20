FROM cgpanel-integration
RUN apt-get update \
    && echo 'postfix postfix/main_mailer_type select Local only' | debconf-set-selections \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
       postfix dovecot-core dovecot-imapd dovecot-lmtpd rspamd redis-server openssl \
    && rm -rf /var/lib/apt/lists/*
