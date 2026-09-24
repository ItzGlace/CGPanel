export function features(ctx) {
  const {
    api,
    esc,
    glyph,
    modal,
    input,
    textarea,
    select,
    toast,
    getMe,
    getCurrent,
  } = ctx;
  const $ = (s) => document.querySelector(s);
  const panel = () => $("#content");
  const button = (text, action, id = "", primary = false) =>
    `<button type="button" data-v2="${action}" data-id="${esc(id)}" class="inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2.5 text-xs font-medium transition duration-200 motion-reduce:transition-none motion-safe:hover:-translate-y-0.5 active:scale-[.98] ${primary ? "bg-forest text-white hover:bg-emerald-900" : "border border-slate-200 bg-white text-slate-600 hover:border-emerald-300"}">${text}</button>`;
  const card = (title, body, extra = "") =>
    `<section class="mb-5 rounded-xl border border-slate-200/70 bg-white p-5 shadow-xs motion-safe:animate-enter ${extra}"><h2 class="mb-4 text-sm font-semibold text-slate-800">${title}</h2>${body}</section>`;
  const note = (text) =>
    `<p class="mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/75">${text}</p>`;
  const empty = (text) =>
    `<p class="py-8 text-center text-sm text-slate-500">${text}</p>`;
  const date = (n) => (n ? new Date(n * 1000).toLocaleString() : "—");
  const size = (n) =>
    n > 1073741824
      ? (n / 1073741824).toFixed(2) + " GB"
      : (n / 1048576).toFixed(2) + " MB";
  const badge = (s) =>
    `<span class="inline-flex rounded-md px-2.5 py-1 text-[11px] font-medium ${["up", "succeeded", "active"].includes(s) ? "bg-emerald-50 text-emerald-700" : ["down", "failed", "interrupted"].includes(s) ? "bg-red-50 text-red-600" : "bg-slate-100 text-slate-500"}">${esc(s)}</span>`;
  const table = (head, rows) =>
    `<div class="overflow-x-auto"><table class="w-full text-left text-xs [&_th]:border-b [&_th]:border-slate-100 [&_th]:px-3 [&_th]:py-3 [&_th]:font-medium [&_th]:text-slate-400 [&_td]:border-b [&_td]:border-slate-50 [&_td]:px-3 [&_td]:py-4 [&_td]:text-slate-600"><thead><tr>${head.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows.join("")}</tbody></table></div>`;
  const row = (cells) =>
    `<tr>${cells.map((c) => `<td>${c}</td>`).join("")}</tr>`;
  const checkbox = (name, label, checked = false) =>
    `<label class="!flex items-center gap-3 text-xs"><input class="!mt-0 !size-4 !shrink-0 !p-0 accent-emerald-700" type="checkbox" name="${name}" ${checked ? "checked" : ""}>${label}</label>`;
  const pick = (name, label, values, value) =>
    select(name, label, values).replace(
      `value="${esc(value)}"`,
      `value="${esc(value)}" selected`,
    );
  const optional = (html) => html.replaceAll(" required", "");
  const close = () => $("#dialog").close();
  let owner = "",
    domains = [],
    apps = [],
    integrations = [],
    jobs = [],
    backups = [],
    selected = "";
  const ownerQuery = () => "?owner=" + encodeURIComponent(owner || getMe().id);
  async function connections(forOwner = owner) {
    return api(
      "/v2/integrations?owner=" + encodeURIComponent(forOwner || getMe().id),
    );
  }
  async function accounts() {
    if (!owner) owner = getMe().id;
    if (getMe().role !== "admin") return "";
    const users = await api("/users");
    if (!users.some((u) => u.id === owner && u.enabled)) owner = getMe().id;
    return `<label class="text-xs text-slate-500">Account <select id="v2-owner" class="ml-3 rounded-lg border border-slate-200 bg-white px-4 py-2.5 text-slate-700">${users
      .filter((u) => u.enabled)
      .map(
        (u) =>
          `<option value="${u.id}" ${u.id === owner ? "selected" : ""}>${esc(u.username)}</option>`,
      )
      .join("")}</select></label>`;
  }
  function domainPicker() {
    selected = sessionStorage.getItem("cgp-domain") || domains[0]?.id || "";
    if (!domains.some((d) => d.id === selected))
      selected = domains[0]?.id || "";
    return `<label class="text-xs text-slate-500">Website <select id="v2-domain" class="ml-3 max-w-full rounded-lg border border-slate-200 bg-white px-4 py-2.5 text-slate-700">${domains.map((d) => `<option value="${d.id}" ${d.id === selected ? "selected" : ""}>${esc(d.name)}</option>`).join("")}</select></label>`;
  }
  const toolbar = (left, right = "") =>
    `<div class="mb-6 flex flex-wrap items-center justify-between gap-3">${left}<div class="flex flex-wrap gap-2">${right}</div></div>`;
  async function queued(kind, target, data = {}) {
    await api("/v2/jobs", "POST", { kind, target, owner, ...data });
    close();
    toast("Job queued. Track its progress in Jobs.");
    location.hash = "jobs";
  }
  function sparkline(checks) {
    if (!checks.length)
      return empty("Monitoring samples will appear after the first check.");
    const samples = [...checks].reverse(),
      max = Math.max(100, ...samples.map((s) => s.latency_ms));
    const points = samples
      .map(
        (v, i) =>
          `${30 + (i / Math.max(1, samples.length - 1)) * 600},${150 - (v.latency_ms / max) * 110}`,
      )
      .join(" ");
    return `<svg viewBox="0 0 650 185" role="img" aria-label="Response time in milliseconds over the latest monitoring samples" class="w-full"><path d="M30 20V150H630" class="fill-none stroke-slate-200"/><polyline points="${points}" class="fill-none stroke-emerald-500" stroke-width="2.5"/><text x="30" y="175" class="fill-slate-400 text-[11px]">Earlier</text><text x="587" y="175" class="fill-slate-400 text-[11px]">Latest</text><text x="34" y="18" class="fill-slate-500 text-[11px]">${max} ms</text></svg>`;
  }
  async function monitoring() {
    domains = await api("/resources/domains");
    if (!domains.length) {
      panel().innerHTML = note(
        "Assign a domain to an application before enabling website monitoring.",
      );
      return;
    }
    const chooser = domainPicker(),
      data = await api("/v2/monitor/" + selected),
      settings = data.settings,
      latest = data.checks[0];
    const uptime = data.last_day.samples
      ? ((data.last_day.up / data.last_day.samples) * 100).toFixed(2) + "%"
      : "No samples";
    panel().innerHTML =
      toolbar(
        chooser,
        button("Configure", "monitor-config", selected, true) +
          button("Refresh", "refresh"),
      ) +
      `<div class="mb-6 grid gap-4 sm:grid-cols-3">${card("Current status", badge(settings.state) + `<p class="mt-3 text-xs text-slate-400">${latest ? date(latest.at) : "Awaiting first check"}</p>`)}${card("Availability · 24 hours", `<p class="text-3xl font-medium text-forest">${uptime}</p><p class="mt-3 text-xs text-slate-400">${data.last_day.samples} checks from this server</p>`)}${card("Latest response", `<p class="text-3xl font-medium text-forest">${latest?.latency_ms || 0}<span class="ml-2 text-sm text-slate-400">ms</span></p><p class="mt-3 text-xs text-slate-400">${latest ? "HTTP " + latest.status : "No measurements yet"}</p>`)}</div>` +
      card("Response time", sparkline(data.checks)) +
      card(
        "Recent checks",
        data.checks.length
          ? table(
              ["Checked", "HTTP", "Response", "Result"],
              data.checks
                .slice(0, 20)
                .map((c) =>
                  row([
                    date(c.at),
                    c.status || "—",
                    c.latency_ms + " ms",
                    c.error
                      ? esc(c.error)
                      : badge(
                          c.status >= 200 && c.status < 400 ? "up" : "down",
                        ),
                  ]),
                ),
            )
          : empty("Enable monitoring to begin."),
      ) +
      note(
        "An incident is opened after two failed checks. Your connected Telegram bot receives outage and recovery messages. Availability reflects checks from this server, not a global monitoring network.",
      );
  }
  async function monitorConfig(id) {
    const domain = domains.find((d) => d.id === id),
      { settings } = await api("/v2/monitor/" + id),
      c = settings.config;
    const connections = await api("/v2/integrations?owner=" + domain.owner);
    modal(
      "Website monitoring",
      checkbox("enabled", "Enable availability checks", settings.enabled) +
        pick(
          "scheme",
          "Protocol",
          [
            ["https", "HTTPS"],
            ["http", "HTTP"],
          ],
          c.scheme,
        ) +
        input(
          "path",
          "Page to check",
          "text",
          "Path only, without query parameters.",
          c.path || "/",
        ) +
        input(
          "interval",
          "Check interval (seconds)",
          "number",
          "60–3600 seconds.",
          c.interval || 60,
        ) +
        pick(
          "telegram_id",
          "Telegram notifications",
          [
            ["", "No Telegram alerts"],
            ...connections
              .filter((i) => i.type === "telegram")
              .map((i) => [i.id, i.name]),
          ],
          c.telegram_id,
        ) +
        checkbox("analytics", "Enable visitor analytics", c.analytics) +
        checkbox("clicks", "Enable click tracking", c.clicks) +
        input(
          "retention_days",
          "Analytics retention (days)",
          "number",
          "1–30 days. Tracking respects Do Not Track and does not collect form contents.",
          c.retention_days || 30,
        ),
      "Save",
      async (v) => {
        await api("/v2/monitor/" + id, "POST", {
          ...v,
          enabled: v.enabled === "on",
          analytics: v.analytics === "on",
          clicks: v.clicks === "on",
          interval: Number(v.interval),
          retention_days: Number(v.retention_days),
        });
        close();
        await render("monitoring");
      },
    );
  }
  async function analytics() {
    domains = await api("/resources/domains");
    if (!domains.length) {
      panel().innerHTML = note("Assign a domain before collecting analytics.");
      return;
    }
    const chooser = domainPicker(),
      path = sessionStorage.getItem("cgp-heatmap-path") || "",
      viewport = sessionStorage.getItem("cgp-heatmap-viewport") || "desktop";
    const [data, monitor] = await Promise.all([
      api(
        "/v2/analytics/" +
          selected +
          (path ? "?path=" + encodeURIComponent(path) : "?") +
          "&viewport=" +
          viewport,
      ),
      api("/v2/monitor/" + selected),
    ]);
    const today = new Date().toISOString().slice(0, 10),
      now = data.daily.find((d) => d.day === today),
      max = Math.max(1, ...data.daily.map((d) => d.views));
    const bars = data.daily
      .map(
        (d, i) =>
          `<rect x="${35 + (i * 560) / Math.max(1, data.daily.length)}" y="${145 - (d.views / max) * 110}" width="${Math.max(2, 450 / Math.max(1, data.daily.length))}" height="${(d.views / max) * 110}" rx="2" class="fill-emerald-400"><title>${esc(d.day)}: ${d.views} views, ${d.unique_ips} unique IPs</title></rect>`,
      )
      .join("");
    const maxClicks = Math.max(1, ...data.heatmap.cells.map((c) => c.count)),
      colors = [
        "bg-emerald-50",
        "bg-emerald-100",
        "bg-emerald-200",
        "bg-emerald-300",
        "bg-emerald-500",
        "bg-emerald-700",
      ];
    const cells = Array.from({ length: 200 }, (_, i) => {
      const cell = data.heatmap.cells.find(
          (c) => c.col === i % 10 && c.row === Math.floor(i / 10),
        ),
        count = cell?.count || 0;
      return `<div class="aspect-[2/1] rounded-sm ${colors[count ? Math.max(1, Math.ceil((count / maxClicks) * 5)) : 0]}" title="${count} clicks" aria-label="Row ${Math.floor(i / 10) + 1}, column ${(i % 10) + 1}: ${count} clicks"></div>`;
    }).join("");
    const snippet = `<script defer src="/__cgpanel/tracker.js" data-site="${selected}" data-key="${monitor.settings.analytics_key || "ENABLE_ANALYTICS_FIRST"}"></script>`;
    panel().innerHTML =
      toolbar(
        chooser,
        button("Tracking setup", "tracking-setup", selected) +
          button("Run SEO check", "seo", selected, true),
      ) +
      `<div class="grid gap-4 sm:grid-cols-3">${card("Page views · today", `<p class="text-3xl font-medium text-forest">${now?.views || 0}</p>`)}${card("Unique IPs · today", `<p class="text-3xl font-medium text-forest">${now?.unique_ips || 0}</p>`)}${card("Clicks · selected page", `<p class="text-3xl font-medium text-forest">${data.heatmap.cells.reduce((a, c) => a + c.count, 0)}</p>`)}</div>` +
      card(
        "Page views · last 30 days",
        data.daily.length
          ? `<svg viewBox="0 0 650 175" role="img" aria-label="Daily page views for the last 30 days" class="w-full"><path d="M30 20V145H630" class="fill-none stroke-slate-200"/>${bars}<text x="35" y="168" class="fill-slate-500 text-[11px]">${data.daily[0].day}</text><text x="555" y="168" class="fill-slate-500 text-[11px]">${data.daily.at(-1).day}</text></svg>`
          : empty(
              "No visits recorded yet. Add the tracking script to your website.",
            ),
      ) +
      `<div class="grid gap-5 lg:grid-cols-2">${card("Click heatmap", `<div class="mb-4 flex flex-wrap gap-3"><select id="heatmap-path" class="max-w-full rounded-lg border border-slate-200 px-3 py-2 text-xs">${[...new Set([data.heatmap.path, ...data.pages.map((p) => p.path)])].map((p) => `<option value="${esc(p)}" ${p === data.heatmap.path ? "selected" : ""}>${esc(p || "/")}</option>`).join("")}</select><select id="heatmap-viewport" class="rounded-lg border border-slate-200 px-3 py-2 text-xs"><option ${viewport === "desktop" ? "selected" : ""}>desktop</option><option ${viewport === "mobile" ? "selected" : ""}>mobile</option></select></div><div class="grid grid-cols-10 gap-1" role="img" aria-label="Click concentration by relative page position; darker cells mean more clicks">${cells}</div><p class="mt-3 text-xs text-slate-500">Top to bottom of the page · darker means more clicks. Layout changes can shift historical positions.</p>`)}${card(
        "Most clicked elements",
        data.targets.length
          ? table(
              ["Element label", "Clicks"],
              data.targets.map((t) => row([esc(t.label), t.count])),
            )
          : empty("No click events recorded."),
      )}</div>` +
      card(
        "Popular pages",
        data.pages.length
          ? table(
              ["Page", "Views", "Daily unique IPs, summed"],
              data.pages.map((p) =>
                row([esc(p.path), p.views, p.unique_ip_days]),
              ),
            )
          : empty("No page views recorded."),
      ) +
      card(
        "Referring websites",
        data.referrers.length
          ? table(
              ["Referrer", "Views"],
              data.referrers.map((r) =>
                row([esc(r.host || "Direct / unavailable"), r.count]),
              ),
            )
          : empty("No referrer data yet."),
      ) +
      note(
        "Unique IPs estimate visitors: shared networks can combine people, and changing IPs can count one person more than once. IP identifiers are hashed separately for each site and day. Analytics require your site's consent setup where applicable.",
      );
    panel().dataset.snippet = snippet;
  }
  async function integrationPage() {
    const account = await accounts();
    integrations = await connections();
    panel().innerHTML =
      toolbar(account, button("Add integration", "integration-add", "", true)) +
      note(
        "Connect a Telegram bot, S3-compatible bucket, SSH server, SOCKS5 proxy, or Cloudflare account. Credentials are stored on the server and never shown again. Test sends a Telegram message or uploads a small archive for storage connections.",
      ) +
      card(
        "Your integrations",
        integrations.length
          ? table(
              ["Name", "Type", "Connection", "Actions"],
              integrations.map((i) =>
                row([
                  esc(i.name),
                  esc(i.type),
                  esc(i.host || i.endpoint || i.chat_id || "Configured"),
                  `<div class="flex gap-2">${button("Test", "integration-test", i.id)}${button("Edit", "integration-edit", i.id)}${button("Remove", "integration-remove", i.id)}</div>`,
                ]),
              ),
            )
          : empty(
              "Add your first integration to enable alerts, remote backups, or proxy routing.",
            ),
      );
  }
  async function integrationForm(id = "", type = "") {
    const item = integrations.find((i) => i.id === id) || {};
    if (!type && !id) {
      modal(
        "Add integration",
        select("type", "Service", [
          ["telegram", "Telegram bot"],
          ["s3", "S3-compatible storage"],
          ["ssh", "SSH / SFTP storage"],
          ["proxy", "SOCKS5 proxy"],
          ["cloudflare", "Cloudflare DNS"],
        ]),
        "Continue",
        async (v) => {
          close();
          await integrationForm("", v.type);
        },
      );
      return;
    }
    type = type || item.type;
    const secret = (key, label, help = "") =>
      optional(
        input(
          key,
          label,
          "password",
          id ? "Leave blank to keep the saved value. " + help : help,
        ),
      );
    let fields = input("name", "Name", "text", "", item.name || "");
    if (type === "proxy")
      fields += secret(
        "url",
        "SOCKS5 URL",
        "socks5h://username:password@host:port. Application gateways require an IPv4 endpoint.",
      );
    if (type === "telegram")
      fields +=
        secret(
          "token",
          "Bot token",
          "Create a bot with Telegram’s @BotFather.",
        ) +
        input(
          "chat_id",
          "Chat ID",
          "text",
          "Start a conversation with your bot or add it to the target group.",
          item.chat_id || "",
        );
    if (type === "s3")
      fields +=
        input(
          "endpoint",
          "HTTPS endpoint",
          "url",
          "Example: https://s3.us-east-1.amazonaws.com",
          item.endpoint || "",
        ) +
        input("bucket", "Bucket", "text", "", item.bucket || "") +
        input(
          "region",
          "Region",
          "text",
          "Use the region required by your storage provider.",
          item.region || "us-east-1",
        ) +
        optional(
          input(
            "prefix",
            "Object prefix",
            "text",
            "Optional folder prefix.",
            item.prefix || "cgpanel",
          ),
        ) +
        secret("access_key", "Access key") +
        secret("secret_key", "Secret key") +
        textarea(
          "ca_pem",
          "Custom CA certificate (optional)",
          "For a private certificate authority. TLS verification remains enabled.",
        );
    if (type === "ssh")
      fields +=
        input(
          "host",
          "Server hostname / IP",
          "text",
          "Publicly reachable SSH server.",
          item.host || "",
        ) +
        input("port", "Port", "number", "", item.port || 22) +
        input("username", "SSH username", "text", "", item.username || "") +
        input(
          "remote_dir",
          "Destination directory",
          "text",
          "Absolute path, for example /home/backup/cgpanel.",
          item.remote_dir || "",
        ) +
        textarea(
          "private_key",
          "Private key",
          id
            ? "Leave blank to retain the saved key."
            : "Unencrypted OpenSSH key dedicated to backups.",
        ) +
        textarea(
          "host_key",
          "Verified server host key",
          "Format: ssh-ed25519 AAAA… Obtain it from the server owner; it will be pinned.",
        );
    if (type === "cloudflare")
      fields += secret(
        "token",
        "Cloudflare API token",
        "Limit the token to DNS editing for the required zone.",
      );
    if (["telegram", "s3", "ssh"].includes(type))
      fields += pick(
        "proxy_id",
        "Outgoing SOCKS proxy",
        [
          ["", "Direct connection"],
          ...integrations
            .filter((i) => i.type === "proxy" && i.id !== id)
            .map((i) => [i.id, i.name]),
        ],
        item.proxy_id,
      );
    modal(
      id ? "Edit integration" : "Connect " + type,
      fields,
      "Save integration",
      async (value) => {
        await api("/v2/integrations", "POST", {
          ...value,
          type,
          id,
          owner,
          port: Number(value.port || 22),
        });
        close();
        await render("integrations");
      },
    );
  }
  async function backupCenter() {
    const account = await accounts();
    [backups, apps] = await Promise.all([
      api("/v2/backups" + ownerQuery()),
      api("/resources/apps"),
    ]);
    const plans = await api("/v2/backup-plans");
    panel().innerHTML =
      toolbar(
        account,
        button("Upload .cgp", "backup-import") + button("Automatic backups", "backup-plan") +
          button("Full .cgp backup", "backup-create", "", true),
      ) +
      note(
        "A .cgp archive contains website files, logical database dumps, application configuration, domains and DNS records. It can contain passwords and API keys. Only send it to storage you control.",
      ) +
      card(
        "Full backups",
        backups.length
          ? table(
              [
                "Application",
                "Created",
                "Size / databases",
                "Delivery",
                "Actions",
              ],
              backups
                .sort((a, b) => b.created - a.created)
                .map((b) =>
                  row([
                    esc(b.name),
                    date(b.created),
                    `${size(b.bytes)} · ${b.database_count} SQL`,
                    b.delivery.length
                      ? b.delivery
                          .map((d) => badge(d.success ? "succeeded" : "failed"))
                          .join(" ")
                      : "Local",
                    `<div class="flex gap-2"><a class="rounded-lg border border-slate-200 px-3 py-2 text-xs" href="/api/v2/backups/${b.id}/download${ownerQuery()}">Download</a>${button("Send", "backup-send", b.id)}${button("Restore", "backup-restore", b.id)}${button("Delete", "backup-delete", b.id)}</div>`,
                  ]),
                ),
            )
          : empty(
              "Create a full backup to protect this website and its databases.",
            ),
      ) +
      card(
        "Automatic schedules",
        plans.length
          ? table(
              ["Application", "Cron / timezone", "Next run", "Actions"],
              plans
                .filter((p) => p.owner === owner)
                .map((p) =>
                  row([
                    esc(
                      apps.find((a) => a.id === p.app_id)?.name ||
                        p.app_id.slice(0, 12),
                    ),
                    `${esc(p.config.schedule)} · ${esc(p.config.timezone || "UTC")}`,
                    date(p.next_run),
                    button("Remove plan", "plan-remove", p.id),
                  ]),
                ),
            )
          : empty("No automatic backup plans."),
      ) +
      note(
        "Local retention applies after a successful scheduled backup. Telegram archives larger than 45 MiB are split into ordered parts with a reconstruction manifest. Remote storage retention remains under your provider's lifecycle policy.",
      );
  }
  async function backupForm(scheduled = false) {
    const allApps = (await api("/resources/apps")).filter(
        (a) => a.owner === owner,
      ),
      dbs = (await api("/resources/databases")).filter(
        (d) => d.owner === owner,
      );
    integrations = await connections();
    if (!allApps.length) {
      toast("Create an application for this account first.");
      return;
    }
    const storage = integrations.filter((i) =>
      ["telegram", "s3", "ssh"].includes(i.type),
    );
    const choose = (name, items) =>
      `<div class="grid gap-2">${items.map((i) => checkbox(`${name}_${i.id}`, esc(i.name) + (i.data?.engine ? " · " + esc(i.data.engine) : ""))).join("") || '<p class="text-xs text-slate-400">None configured</p>'}</div>`;
    let fields =
      select(
        "app_id",
        "Application",
        allApps.map((a) => [a.id, a.name]),
      ) +
      `<p class="text-xs font-medium">Include databases</p>` +
      choose("database", dbs) +
      `<p class="text-xs font-medium">Send to storage</p>` +
      choose("destination", storage) +
      checkbox(
        "quiesce",
        "Pause the application while capturing files and SQL",
        true,
      );
    if (scheduled)
      fields +=
        input(
          "schedule",
          "Cron expression",
          "text",
          "Five fields: minute hour day month weekday.",
          "0 3 * * *",
        ) +
        input(
          "timezone",
          "Timezone",
          "text",
          "IANA timezone, for example Asia/Tehran.",
          "UTC",
        ) +
        input(
          "retention",
          "Keep local copies",
          "number",
          "1–30 successful scheduled backups.",
          7,
        );
    modal(
      scheduled ? "Automatic full backups" : "Create full .cgp backup",
      fields,
      scheduled ? "Save plan" : "Create backup",
      async (value) => {
        const data = {
          app_id: value.app_id,
          database_ids: dbs
            .filter((d) => value["database_" + d.id] === "on")
            .map((d) => d.id),
          destinations: storage
            .filter((d) => value["destination_" + d.id] === "on")
            .map((d) => d.id),
          quiesce: value.quiesce === "on",
          schedule: value.schedule,
          timezone: value.timezone,
          retention: Number(value.retention || 7),
        };
        if (scheduled) {
          await api("/v2/backup-plans", "POST", data);
          close();
          await render("backup-center");
        } else await queued("full_backup", value.app_id, data);
      },
    );
  }
  async function certificates() {
    domains = await api("/resources/domains");
    const chooser = domains.length
      ? domainPicker()
      : "<span class='text-xs text-slate-500'>No assigned domains</span>";
    const data = domains.length
      ? await api("/v2/domains/" + selected + "/tls")
      : {};
    panel().innerHTML =
      toolbar(
        chooser,
        (getMe().role === "admin"
          ? button("Panel IP certificate", "panel-tls")
          : "") +
          (domains.length
            ? button("Request HTTPS", "tls", selected, true)
            : ""),
      ) +
      (domains.length
        ? card(
            "Certificate and renewal",
            `<div class="mb-4 flex gap-3">${badge(data.installed ? "active" : "not installed")}${badge(data.renewal_timer_active ? "renewal timer active" : "renewal timer unavailable")}</div><pre class="overflow-auto whitespace-pre-wrap rounded-lg bg-slate-50 p-4 text-xs leading-6 text-slate-600">${esc(data.certificate || "No certificate installed for this domain.")}</pre><p class="mt-4 text-xs text-slate-500">${esc(data.last_renewal_run || "")}</p>`,
          )
        : card(
            "Domain certificates",
            empty(
              "Assign a domain to an application to manage its certificate and renewal status.",
            ),
          )) +
      (domains.length
        ? card(
            "CDN and DNS export",
            `<p class="mb-4 text-xs leading-6 text-slate-500">Download a BIND zone file and import it into Cloudflare or another DNS provider. Complete nameserver delegation and CDN activation with your provider. Cloudflare mode trusts visitor-IP headers only from Cloudflare's published networks.</p><div class="flex flex-wrap gap-3"><a class="rounded-lg border border-slate-200 px-4 py-2.5 text-xs" href="/api/v2/domains/${selected}/zone">Download DNS zone</a>${button("CDN settings", "cdn", selected)}</div>`,
          )
        : "") +
      note(
        "HTTP validation requires port 80 to reach this server. Local DNS validation requires your domain to use this server's authoritative DNS. Cloudflare DNS validation uses a scoped API token. Renewals run automatically every six hours; staging certificates are for testing and do not replace the active certificate.",
      );
  }
  async function tlsForm(id, panelIP = false) {
    const domain = panelIP
      ? null
      : (await api("/resources/domains")).find((d) => d.id === id);
    const connections = panelIP
      ? []
      : await api("/v2/integrations?owner=" + domain.owner);
    let fields = input("email", "Certificate contact email", "email");
    if (!panelIP)
      fields +=
        select("validation", "Verification", [
          ["http", "HTTP through this server's IP"],
          ["dns_local", "DNS on this authoritative server"],
          ["dns_cloudflare", "Cloudflare DNS"],
        ]) +
        select("integration_id", "Cloudflare integration", [
          ["", "Not required for HTTP / local DNS"],
          ...connections
            .filter((i) => i.type === "cloudflare")
            .map((i) => [i.id, i.name]),
        ]);
    fields +=
      checkbox("staging", "Use the Let's Encrypt staging environment") +
      checkbox(
        "agree_tos",
        'I accept the <a class="underline" href="https://letsencrypt.org/repository/" target="_blank" rel="noopener">Let’s Encrypt subscriber agreement</a>',
      );
    modal(
      panelIP ? "Secure the panel's public IP" : "Free HTTPS certificate",
      fields,
      "Request certificate",
      async (v) => {
        if (v.agree_tos !== "on")
          throw new Error("Accept the subscriber agreement to continue.");
        await queued(panelIP ? "panel_tls" : "tls", panelIP ? getMe().id : id, {
          ...v,
          agree_tos: true,
          staging: v.staging === "on",
        });
      },
    );
  }
  async function egress() {
    apps = await api("/resources/apps");
    panel().innerHTML =
      note(
        "Route an application's TCP connections and DNS through a SOCKS5 proxy. Ordinary PHP and other runtime requests use the gateway automatically. Unsupported UDP and IPv6 traffic is blocked. If the proxy fails, traffic cannot fall back to a direct connection.",
      ) +
      card(
        "Application routing",
        apps.length
          ? table(
              ["Application", "Runtime", "Actions"],
              apps.map((a) =>
                row([
                  esc(a.name),
                  esc(a.data.runtime),
                  button("Proxy settings", "egress", a.id, true),
                ]),
              ),
            )
          : empty("Create an application first."),
      );
  }
  async function egressForm(id) {
    const app = apps.find((a) => a.id === id),
      [state, connections] = await Promise.all([
        api("/v2/egress/" + id),
        api("/v2/integrations?owner=" + app.owner),
      ]);
    if (state.locked && getMe().role !== "admin") {
      toast("An administrator has locked this application's proxy settings.");
      return;
    }
    modal(
      "Outgoing proxy · " + app.name,
      pick(
        "proxy_id",
        "Route connections through",
        [
          ["", "Direct connection"],
          ...connections
            .filter((i) => i.type === "proxy")
            .map((i) => [i.id, i.name]),
        ],
        state.proxy_id,
      ) +
        (getMe().role === "admin"
          ? checkbox(
              "locked",
              "Lock this setting against tenant changes",
              state.locked,
            )
          : "") +
        `<p class="text-xs leading-6 text-slate-500">Applying this change recreates the application container and briefly interrupts the website. Workspace files, environment variables and the website port are retained.</p>`,
      "Apply routing",
      async (v) =>
        queued("egress", id, {
          proxy_id: v.proxy_id,
          locked: v.locked === "on",
        }),
    );
  }
  async function jobPage() {
    jobs = await api("/v2/jobs");
    panel().innerHTML =
      toolbar(
        `<span class="text-xs text-slate-500">Recent background operations</span>`,
        button("Refresh", "refresh"),
      ) +
      card(
        "Jobs",
        jobs.length
          ? table(
              ["Job", "Status", "Started", "Result", "Actions"],
              jobs.map((j) =>
                row([
                  esc(j.kind.replaceAll("_", " ")),
                  badge(j.status),
                  date(j.started || j.created),
                  esc(
                    j.error ||
                      (j.status === "succeeded"
                        ? "Completed"
                        : "Waiting for completion"),
                  ),
                  button("Details", "job-details", j.id) +
                    (["failed", "interrupted"].includes(j.status)
                      ? " " + button("Retry", "job-retry", j.id)
                      : ""),
                ]),
              ),
            )
          : empty("Background operations will appear here."),
      );
  }
  async function render(page) {
    if (page === "monitoring") await monitoring();
    else if (page === "analytics") await analytics();
    else if (page === "integrations") await integrationPage();
    else if (page === "backup-center") await backupCenter();
    else if (page === "certificates") await certificates();
    else if (page === "egress") await egress();
    else if (page === "jobs") await jobPage();
  }
  document.addEventListener("change", async (event) => {
    try {
      if (event.target.id === "v2-owner") {
        owner = event.target.value;
        await render(getCurrent());
      }
      if (event.target.id === "v2-domain") {
        sessionStorage.setItem("cgp-domain", event.target.value);
        sessionStorage.removeItem("cgp-heatmap-path");
        await render(getCurrent());
      }
      if (event.target.id === "heatmap-path") {
        sessionStorage.setItem("cgp-heatmap-path", event.target.value);
        await analytics();
      }
      if (event.target.id === "heatmap-viewport") {
        sessionStorage.setItem("cgp-heatmap-viewport", event.target.value);
        await analytics();
      }
    } catch (error) {
      toast(error.message);
    }
  });
  document.addEventListener("click", async (event) => {
    const b = event.target.closest("[data-v2]");
    if (!b) return;
    const action = b.dataset.v2,
      id = b.dataset.id;
    try {
      if (action === "refresh") await render(getCurrent());
      if (action === "monitor-config") await monitorConfig(id);
      if (action === "tracking-setup")
        modal(
          "Install visitor tracking",
          `<p class="mb-4 text-xs leading-6 text-slate-500">Enable analytics in Monitoring, then add this script to pages you want to measure. Load it after any consent required for your website. Add data-cgp-label="pricing-button" to an element to name it in click reports.</p><pre class="overflow-auto whitespace-pre-wrap break-all rounded-lg bg-slate-50 p-4 text-xs">${esc(panel().dataset.snippet)}</pre>`,
          "",
          null,
        );
      if (action === "seo") {
        const m = await api("/v2/monitor/" + id);
        await queued("seo", id, {
          scheme: m.settings.config.scheme || "https",
        });
      }
      if (action === "integration-add") await integrationForm();
      if (action === "integration-edit") await integrationForm(id);
      if (action === "integration-test") await queued("integration_test", id);
      if (action === "integration-remove")
        modal(
          "Remove integration",
          `<p class="text-xs">Remove this saved connection? Linked connections must be detached first.</p>`,
          "Remove",
          async () => {
            await api("/v2/integrations/" + id + ownerQuery(), "DELETE");
            close();
            await integrationPage();
          },
        );
      if(action==='backup-import'){
        const choices=apps.filter(a=>a.owner===owner);
        if(!choices.length)throw new Error('Choose an account with an application first.');
        modal('Upload a .cgp backup',select('app_id','Original application',choices.map(a=>[a.id,a.name]))+'<label class="block text-xs text-slate-500">Archive<input id="backup-upload" type="file" accept=".cgp" required class="my-3 block w-full rounded-lg border border-slate-200 p-3"></label><p class="text-xs leading-6 text-slate-500">Uploads up to 2 GiB. The archive must belong to this account and application. After upload, choose Restore in the backup list. Restores create a recovery backup before changing files or databases.</p><p id="backup-upload-state" class="mt-3 text-xs text-emerald-700" role="status"></p>','Upload archive',async value=>{
          const file=$('#backup-upload').files[0],status=$('#backup-upload-state');if(!file||!file.name.toLowerCase().endsWith('.cgp'))throw new Error('Choose a .cgp file.');
          const endpoint='/v5/apps/'+value.app_id+'/backup-import';const begin=await api(endpoint,'POST',{operation:'begin',size:file.size});let complete=false;
          try{for(let at=0;at<file.size;){const bytes=new Uint8Array(await file.slice(at,at+196608).arrayBuffer());let raw='';for(let i=0;i<bytes.length;i+=8192)raw+=String.fromCharCode(...bytes.subarray(i,i+8192));const next=await api(endpoint,'POST',{operation:'chunk',upload:begin.upload,offset:at,data:btoa(raw)});at=next.next;status.textContent='Uploading '+Math.round(at/file.size*100)+'%';}await api(endpoint,'POST',{operation:'finish',upload:begin.upload});complete=true;$('#dialog').close();if(getCurrent()==='backup-center')await backupCenter();toast('Archive uploaded. Choose Restore to apply it.');}
          finally{if(!complete)await api(endpoint,'POST',{operation:'cancel',upload:begin.upload}).catch(()=>{});}
        });
      }
      if (action === "backup-create") await backupForm();
      if (action === "backup-plan") await backupForm(true);
      if (action === "plan-remove")
        modal(
          "Remove backup schedule",
          "<p class='text-xs'>Existing backups are retained.</p>",
          "Remove",
          async () => {
            await api("/v2/backup-plans/" + id, "DELETE");
            close();
            await backupCenter();
          },
        );
      if (action === "backup-send") {
        const storage = (await connections()).filter((i) =>
          ["telegram", "ssh", "s3"].includes(i.type),
        );
        modal(
          "Send backup",
          select(
            "destination",
            "Destination",
            storage.map((i) => [i.id, i.name]),
          ),
          "Send",
          async (v) => queued("backup_deliver", id, v),
        );
      }
      if (action === "backup-delete")
        modal(
          "Delete local backup",
          `<p class="text-xs">Delete this local .cgp archive? Remote copies remain in your storage.</p>`,
          "Delete",
          async () => {
            await api("/v2/backups/" + id + ownerQuery(), "DELETE");
            close();
            await backupCenter();
          },
        );
      if (action === "backup-restore") {
        const backup = backups.find((b) => b.id === id);
        modal(
          "Restore website backup",
          `<p class="mb-4 text-xs leading-6 text-amber-800">The application, IDE and file-transfer access pause during restore. The workspace is replaced with the archived files, including removing files created later, and selected database contents are overwritten. A fresh recovery backup is created automatically before restoring. Domain ownership stays unchanged; archive domain and DNS metadata remains available for administrator review.</p>` +
            input("confirm", "Type RESTORE"),
          "Restore",
          async (v) =>
            queued("restore_full", backup.app_id, {
              backup_id: id,
              confirm: v.confirm,
            }),
        );
      }
      if (action === "tls") await tlsForm(id);
      if (action === "panel-tls") await tlsForm("", true);
      if (action === "cdn")
        modal(
          "CDN settings",
          select("provider", "Provider", [
            ["none", "Direct / other CDN"],
            ["cloudflare", "Cloudflare"],
          ]),
          "Save",
          async (v) => {
            await api("/v2/domains/" + id + "/cdn", "POST", v);
            close();
            await certificates();
          },
        );
      if (action === "egress") await egressForm(id);
      if (action === "job-retry") {
        await api("/v2/jobs/" + id + "/retry", "POST", {});
        await jobPage();
      }
      if (action === "job-details") {
        const j = jobs.find((j) => j.id === id);
        let content;
        if (j.kind === "seo" && j.result.checks)
          content =
            (j.result.discovery
              ? table(
                  ["Discovery", "HTTP status"],
                  j.result.discovery.map((d) =>
                    row([esc(d.path), d.status || "Unavailable"]),
                  ),
                )
              : "") +
            table(
              ["Check", "Finding"],
              j.result.checks.map((c) =>
                row([`${c.ok ? "✓" : "•"} ${esc(c.name)}`, esc(c.detail)]),
              ),
            ) +
            `<p class="mt-4 text-xs text-slate-500">${esc(j.result.note)} Response: ${j.result.response_ms} ms.</p>`;
        else
          content = `<pre class="overflow-auto whitespace-pre-wrap break-all rounded-lg bg-slate-50 p-4 text-xs leading-6">${esc(j.error || JSON.stringify(j.result, null, 2))}</pre>`;
        modal("Job · " + j.kind.replaceAll("_", " "), content, "", null);
      }
    } catch (error) {
      toast(error.message);
    }
  });
  setInterval(() => {
    if (getCurrent() === "jobs" && !$("#dialog").open)
      jobPage().catch(() => {});
  }, 10000);
  return { render, tlsForm };
}
