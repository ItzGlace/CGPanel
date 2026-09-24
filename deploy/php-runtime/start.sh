#!/bin/bash
set -euo pipefail
umask 0027
mkdir -p /workspace/var/sessions /tmp/nginx-client /tmp/nginx-fastcgi /tmp/nginx-proxy
php-fpm --nodaemonize --fpm-config /opt/lunar/php-fpm.conf &
fpm_pid=$!
nginx -c /opt/lunar/nginx.conf -g 'daemon off;' &
nginx_pid=$!
trap 'kill "$fpm_pid" "$nginx_pid" 2>/dev/null || true; wait || true' EXIT
trap 'exit 0' TERM INT QUIT
wait -n "$fpm_pid" "$nginx_pid"
exit 1
