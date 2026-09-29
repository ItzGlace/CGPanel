# IDE routing behind a CDN

The default IDE port is 8443. The CDN must forward this port and WebSocket upgrades. Some providers only forward ports 80 and 443; a running IDE can therefore have an unreachable default link.

For those providers, configure a dedicated IDE hostname on public HTTPS port 443 and proxy it to the application's existing loopback `ide_port`. Preserve the Host header and WebSocket Upgrade/Connection headers, disable caching, and retain code-server password authentication. Keep the IDE on a separate origin from the panel. Restrict an HTTP origin to the CDN and redirect public HTTP to HTTPS using only trusted proxy headers.

The root administrator can override launch URLs in `/etc/cgpanel/ide-public-urls.json`:

```json
{
  "APPLICATION_ID": "https://ide.example.com/"
}
```

Keep this file root-owned with mode 0600. It is read on each IDE status request; no service restart is needed when changing the mapping. This mapping changes the launch link only: configure DNS, TLS and the reverse proxy first. Never reuse a hostname that points to a different application's IDE. Verify login and an authenticated WebSocket upgrade before publishing the route.
