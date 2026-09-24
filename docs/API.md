# Administrator API guide

CGPanel v0.3 adds administrator bearer tokens, a paginated audit API and an in-panel API reference. Existing management endpoints work with either a signed-in session or an appropriately scoped administrator token. These examples target your own panel, not the GitHub API.

## Base URL and formats

Use `https://panel.example.com:2083` (replace it with your panel origin). Paths below include `/api`. Send JSON with `Content-Type: application/json`. IDs are opaque strings returned by creation/list endpoints; use IDs, not usernames or domain names, in `{id}` paths. Successful synchronous JSON operations return HTTP 200, including creates. Background submissions return HTTP 200 with `{ "id": "JOB_ID", "status": "queued" }`; this is not completion.

Use a valid HTTPS certificate or explicitly trust your development CA using `--cacert` / Python's CA context. Do not disable TLS verification in automation. CORS is not enabled for arbitrary third-party browser applications.

## Option 1: administrator bearer token

1. Sign in as administrator and open **Admin API → Create token**.
2. Name the script, choose `read` or `admin`, choose 1–90 days and optionally enter source CIDRs.
3. Copy the secret once to a secret store. The token list shows only metadata and a prefix. Do not put the token in a URL or commit it to Git.

Use `Authorization: Bearer TOKEN`. Bearer requests do not use cookies or CSRF headers. Explicit invalid Authorization headers never fall back to a valid cookie.

| Scope | Access |
| --- | --- |
| `read` | GET and HEAD management requests only. Includes sensitive readable information and backup downloads across tenants. Does not permit POST-based logs, file reads or cron previews. |
| `admin` | Read and mutate managed resources with the administrator's existing permissions. No root-shell endpoint. |
| Either token scope | Cannot list/create/revoke tokens, change the account password or log out browser sessions. Those operations require a session. |

Both account and token IP filters must allow the caller. Disabled accounts, expired/revoked tokens or accounts no longer holding the administrator role cannot use tokens. Password changes revoke all the account's tokens. Mutating bearer calls add an audit entry containing the token ID, method and path; secrets are never included. Revocation stops future requests; it cannot cancel an already running operation or previously queued job.

### curl: read server health

This Bash example keeps the credential out of shell history and curl's command-line arguments. Use a private temporary directory and delete it when finished.

```bash
export CGPANEL_URL='https://panel.example.com:2083'
read -rsp 'API token: ' CGPANEL_TOKEN; printf '\n'
umask 077
CG_API_TMP=$(mktemp -d)
trap 'rm -rf -- "$CG_API_TMP"; unset CGPANEL_TOKEN' EXIT
printf 'Authorization: Bearer %s\n' "$CGPANEL_TOKEN" > "$CG_API_TMP/headers"
unset CGPANEL_TOKEN
curl --fail-with-body --silent --show-error \
  --header @"$CG_API_TMP/headers" "$CGPANEL_URL/api/admin/system"
```

Reuse the header file for subsequent examples. Set `--cacert /path/to/your-ca.pem` when using your own trusted development CA.

### Python: list users without third-party packages

```python
import getpass, json, os, ssl, urllib.request

base = os.environ["CGPANEL_URL"].rstrip("/")
token = getpass.getpass("Administrator API token: ")
context = ssl.create_default_context(cafile=os.getenv("CGPANEL_CA_FILE"))
request = urllib.request.Request(
    base + "/api/users",
    headers={"Authorization": "Bearer " + token, "Accept": "application/json"},
)
with urllib.request.urlopen(request, context=context, timeout=30) as response:
    users = json.load(response)
for user in users:
    print(user["id"], user["username"], user["enabled"])
```

## Option 2: session and CSRF

`POST /api/login` accepts `username` and `password`, sets the HttpOnly `cg_session` cookie and returns `csrf`. Sessions last eight hours. Save the cookie jar and send the returned value in `x-csrf-token` for every non-GET request, including logout and DELETE. `GET /api/me` also returns `csrf`. The current implementation requires CSRF on HEAD with session authentication; use GET unless you supply that header.

Keep login input and output private. In the following Bash example (with curl and jq installed), `login.json` is a mode-0600 file containing your username and password. Do not commit it.

```bash
export CGPANEL_URL='https://panel.example.com:2083'
umask 077
CG_API_TMP=$(mktemp -d)
trap 'rm -rf -- "$CG_API_TMP"' EXIT
curl --fail-with-body --silent --show-error \
  --cookie-jar "$CG_API_TMP/cookies" \
  --header 'Content-Type: application/json' --data-binary @login.json \
  "$CGPANEL_URL/api/login" > "$CG_API_TMP/login-response.json"
jq -r '"x-csrf-token: " + .csrf' "$CG_API_TMP/login-response.json" > "$CG_API_TMP/csrf"
curl --fail-with-body --silent --show-error \
  --cookie "$CG_API_TMP/cookies" --header @"$CG_API_TMP/csrf" \
  --header 'Content-Type: application/json' \
  --data '{"name":"nightly-report","scope":"read","expires_days":30,"allowed_ips":["203.0.113.10/32"]}' \
  "$CGPANEL_URL/api/admin/tokens" > "$CG_API_TMP/new-token.json"
```

The token response contains its secret only this once. Replace the documentation IP with your actual source address, or use an empty list to apply no additional token-level IP filter. Token management is session-only even for full-admin bearer tokens.

## Provision a tenant and website

Use a full-admin token for these mutations. Create a protected `tenant.json` with `username`, a password of 14–256 characters, `quota` and `allowed_ips`. Usernames use lowercase letters, digits and underscores, 3–32 characters. `POST /api/users` creates tenant users only, not additional administrators.

```bash
curl --fail-with-body --silent --show-error --header @"$CG_API_TMP/headers" \
  --header 'Content-Type: application/json' --data-binary @tenant.json \
  "$CGPANEL_URL/api/users"
```

Use the returned tenant ID as `owner` in `POST /api/resources/apps`:

```json
{"owner":"TENANT_ID","name":"website","runtime":"php","mode":"web","command":"","env":{}}
```

An empty command selects the runtime starter. Custom HTTP applications listen on `0.0.0.0:8080`. Save the returned application `id`, then create an assigned domain with `POST /api/resources/domains`:

```json
{"owner":"TENANT_ID","name":"example.com","app_id":"APP_ID"}
```

To create a database, use `POST /api/resources/databases`:

```json
{"owner":"TENANT_ID","name":"website","engine":"mysql","allowed_ips":["203.0.113.10"]}
```

The creation response's `result` contains generated database credentials. Store them securely; list responses do not return the password. PostgreSQL uses `engine: "postgresql"`. Database allowlists require exact IPs; panel/token allowlists use CIDRs.

## Ownership and updates

- Administrators list resources across tenants; ordinary users see their own resources.
- Supply `owner` when creating resources or integrations for a tenant. Omitting it defaults to the caller. For integration/backup list, delete and download operations, supply `?owner=TENANT_ID`; omission defaults to the caller, not every tenant.
- Resource-specific actions infer the owner from the resource ID. Linked applications, domains, databases and integrations must have the same owner.
- Most resources have create/delete/action endpoints, not general PATCH endpoints. Do not assume an undocumented update method exists.
- `POST /api/users/{id}` is a replacement-style access update: send the intended `enabled`, `quota` and `allowed_ips` together. Omitted fields reset to their defaults. An optional nonempty `password` resets that tenant's password. The endpoint revokes their sessions and cannot modify administrator accounts.
- Integration saves reuse an `id` to edit. Blank/omitted secret fields preserve existing secrets; send the complete desired nonsecret fields. Backup plans are created by POST and removed by DELETE, not updated in place.

## Background jobs, monitoring and backups

`POST /api/v2/jobs` accepts a flat object containing `kind`, `target` and job-specific fields. Credentials must be saved in an integration first. Do not put bot tokens, passwords or private keys in job payloads.

Create a full archive:

```json
{"kind":"full_backup","target":"APP_ID","database_ids":["DATABASE_ID"],"destinations":["STORAGE_INTEGRATION_ID"],"quiesce":true}
```

Poll `GET /api/v2/jobs` and select the returned job ID. The list contains the latest 100 accessible jobs. `queued` and `running` are unfinished; `succeeded` is complete; `failed` and `interrupted` require inspection. Poll every 2–5 seconds with an overall deadline. Do not automatically retry creates or restores after a network timeout: a request may already have taken effect. There are no idempotency keys in this release.

List archives with `GET /api/v2/backups?owner=TENANT_ID`, then download the selected ID:

```bash
curl --fail --silent --show-error --header @"$CG_API_TMP/headers" \
  --output website.cgp \
  "$CGPANEL_URL/api/v2/backups/BACKUP_ID/download?owner=TENANT_ID"
```

`GET /api/v2/backups` lists full archives. The legacy `resources/backups` endpoints are workspace snapshots only. A full archive includes files, selected SQL dumps and configuration metadata. The restore job overlays workspace/SQL into existing resources and requires `confirm: "RESTORE"`; domain/DNS/environment metadata recovery is manual.

Enable monitoring with `POST /api/v2/monitor/DOMAIN_ID`:

```json
{"enabled":true,"scheme":"https","path":"/","interval":60,"telegram_id":"TELEGRAM_INTEGRATION_ID","analytics":true,"clicks":true,"retention_days":30}
```

Read status from the same path and analytics from `GET /api/v2/analytics/DOMAIN_ID?path=%2F&viewport=desktop`. Enabling analytics does not inject the tracker into your website; add the script from **Website analytics → Tracking setup**.

For scheduled backups use `POST /api/v2/backup-plans` with `app_id`, `database_ids`, `destinations`, `schedule`, `timezone`, `retention`, `quiesce` and `enabled`. For application commands use `POST /api/resources/schedules`. Their five-field cron/timezone interpretation is shared.

## TLS, CDN and SOCKS examples

Submit a domain certificate job after agreeing to the certificate provider's subscriber agreement:

```json
{"kind":"tls","target":"DOMAIN_ID","email":"admin@example.com","validation":"http","agree_tos":true,"staging":true}
```

Staging tests do not install a trusted certificate. Use `validation: "dns_local"` for delegated local authoritative DNS, or `"dns_cloudflare"` with `integration_id`. The administrator-only `panel_tls` job uses the current administrator ID as `target` and HTTP validation of the public IP. There is no DNS proof for an IP address.

Export `GET /api/v2/domains/DOMAIN_ID/zone` for provider import. Set `POST /api/v2/domains/DOMAIN_ID/cdn` to `{"provider":"cloudflare"}` or `{"provider":"none"}`. Registrar/CDN activation is a separate provider action.

Save a `proxy` integration with a `socks5h://` URL, then submit:

```json
{"kind":"egress","target":"APP_ID","proxy_id":"PROXY_INTEGRATION_ID","locked":true}
```

This recreates the app container with a brief interruption, preserves its workspace/environment/port and enforces TCP/DNS proxy routing. An empty `proxy_id` selects direct routing. Only administrators can lock/unlock it. Other UDP and IPv6 are blocked in proxy mode; a failed proxy cannot fall back to direct traffic.

## Errors and limits

| Status | Meaning |
| --- | --- |
| 200 | Successful synchronous request or job accepted; inspect its JSON/status. |
| 400 | Validation failure, quota, unsupported operation or broker rejection. |
| 401 | Missing, expired or invalid session/token. |
| 403 | Role, IP rule, token scope or session CSRF denial. |
| 404 | Missing or inaccessible resource/document/token. |
| 413 | Request body exceeds its limit. |
| 429 | Login/telemetry throttling or front-proxy rate limit. Back off. |
| 500 | Internal operation failure. Inspect service logs. |

Application errors generally use `{"error":"message"}`. Malformed JSON, method/extractor failures and reverse-proxy errors may return a different body. JSON requests are limited to 512 KiB; public telemetry to 8 KiB. Expensive synchronous provisioning can take minutes; use reasonable client timeouts. Handle errors without logging Authorization headers, cookie jars, integration secrets or archive contents.

## Complete reference

Use **Admin API → Endpoints** or the [endpoint reference](https://github.com/ItzGlace/CGPanel/blob/main/docs/API-REFERENCE.md) for every documented operation, payload example, response shape and ownership rule. Download [OpenAPI JSON](https://github.com/ItzGlace/CGPanel/blob/main/docs/openapi.json) from GitHub or authenticated `GET /api/admin/openapi.json`. The panel and GitHub use the same checked-in documents.
