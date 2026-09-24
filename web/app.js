import { features } from "/features.js";
import { documentation } from "/documentation.js";
import { icon as glyph } from "/icons.js";
("use strict");
const $ = (q, root = document) => root.querySelector(q);
const esc = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
let me,
  current = "home",
  userList = [],
  resources = {},
  csrf = "",
  pending = false;
const pages = {
  docs: [
    "Documentation",
    "Guides for building, operating, and automating your workspace.",
    "folder-code",
  ],
  "admin-api": [
    "Admin API",
    "Endpoint reference and secure access for administrator automation.",
    "terminal",
  ],
  monitoring: [
    "Website monitoring",
    "Availability, response times, and Telegram alerts.",
    "activity",
  ],
  analytics: [
    "Website analytics",
    "Daily unique IPs, popular pages, and click hotspots.",
    "activity",
  ],
  integrations: [
    "Integrations",
    "Connect Telegram, S3, SSH, Cloudflare, and SOCKS5.",
    "network",
  ],
  "backup-center": [
    "Full backups",
    "Portable .cgp archives and scheduled off-site delivery.",
    "archive",
  ],
  certificates: [
    "Certificates & CDN",
    "Let\u2019s Encrypt, automatic renewal, and DNS zone exports.",
    "lock-keyhole",
  ],
  egress: [
    "Outgoing traffic",
    "Route application traffic through an isolated SOCKS5 gateway.",
    "shield-check",
  ],
  jobs: [
    "Background jobs",
    "Track long-running operations and delivery results.",
    "clock-3",
  ],

  home: [
    "Overview",
    "Everything you need to build, deploy, and keep things running.",
    "layout-dashboard",
  ],
  apps: [
    "Applications",
    "A home for websites, Telegram bots, APIs, and background workers.",
    "boxes",
  ],
  domains: [
    "Domains",
    "Assign domains, connect applications, and manage HTTPS.",
    "globe",
  ],
  databases: [
    "Databases",
    "MySQL and PostgreSQL, with a separate identity for each database.",
    "database",
  ],
  dns: [
    "DNS zone editor",
    "Publish records for your assigned domains.",
    "network",
  ],
  files: [
    "File manager",
    "Read and edit application files inside an isolated workspace.",
    "folder-code",
  ],
  terminal: [
    "Terminal",
    "Run commands and install application packages in your container.",
    "terminal",
  ],
  schedules: [
    "Scheduled tasks",
    "Run recurring commands inside your applications.",
    "clock-3",
  ],
  backups: [
    "Backups",
    "Create and restore snapshots of application workspaces.",
    "archive",
  ],
  security: [
    "Security center",
    "Inspect the protections around your hosting environment.",
    "shield-check",
  ],
  blocks: [
    "IP blocklist",
    "Block specific source addresses at the host firewall.",
    "ban",
  ],
  users: [
    "User manager",
    "Create tenant accounts and control panel access.",
    "users",
  ],
  audit: [
    "Activity log",
    "A record of account changes and server operations.",
    "logs",
  ],
  settings: [
    "Account settings",
    "Manage your password and session.",
    "settings",
  ],
};
async function api(path, method = "GET", body) {
  const options = {
    method,
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
  };
  if (method !== "GET") options.headers["x-csrf-token"] = csrf;
  if (body !== undefined) options.body = JSON.stringify(body);
  const response = await fetch("/api" + path, options);
  const data = await response
    .json()
    .catch(() => ({ error: "Unexpected server response" }));
  if (!response.ok) {
    if (response.status === 401 && path !== "/login") showLogin();
    throw new Error(data.error || "Request failed");
  }
  return data;
}
function toast(message) {
  const t = $("#toast");
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), 5000);
}
function showLogin() {
  me = null;
  $("#panel").hidden = true;
  $("#login-screen").hidden = false;
}
async function boot() {
  try {
    me = await api("/me");
    csrf = me.csrf;
    $("#username").textContent = me.username;
    $("#avatar").textContent = me.username[0].toUpperCase();
    $("#login-screen").hidden = true;
    $("#panel").hidden = false;
    nav();
    await render();
  } catch (e) {
    showLogin();
  }
}
$("#login").addEventListener("submit", async (e) => {
  e.preventDefault();
  const b = $("button", e.target);
  b.disabled = true;
  $("#login-error").textContent = "";
  try {
    await api("/login", "POST", Object.fromEntries(new FormData(e.target)));
    e.target.reset();
    await boot();
  } catch (e) {
    $("#login-error").textContent = e.message;
  } finally {
    b.disabled = false;
  }
});
function nav() {
  const keys = [
    "home",
    "apps",
    "domains",
    "databases",
    "dns",
    "files",
    "terminal",
    "schedules",
    "backup-center",
    "monitoring",
    "analytics",
    "certificates",
    "integrations",
    "egress",
    "jobs",
    "security",
    ...(me.role === "admin" ? ["users", "blocks", "admin-api"] : []),
    "audit",
    "docs",
  ];
  $("#nav").innerHTML = keys
    .map(
      (k, i) =>
        `<a href="#${k}" data-nav="${k}" class="flex items-center gap-3 rounded-lg px-3 py-2.5 text-[12px] text-emerald-50/50 transition motion-reduce:transition-none hover:bg-white/5 hover:text-white [&.active]:bg-mint/10 [&.active]:text-mint ${i === 9 ? "nav-group mt-5 border-t border-white/10 pt-5" : ""}"><span class="nav-icon">${glyph(pages[k][2], "size-[18px]")}</span>${pages[k][0]}</a>`,
    )
    .join("");
}
window.addEventListener("hashchange", () => {
  if (me) render();
});
$("#mobile-menu").onclick = () => $(".sidebar").classList.toggle("open");
$("#account-menu").onclick = () => {
  location.hash = "settings";
};
async function load(kind) {
  resources[kind] = await api("/resources/" + kind);
  return resources[kind];
}
async function render() {
  current = location.hash.slice(1) || "home";
  if (!pages[current]) current = "home";
  $(".sidebar").classList.remove("open");
  document
    .querySelectorAll("[data-nav]")
    .forEach((a) => a.classList.toggle("active", a.dataset.nav === current));
  const [title, description] = pages[current];
  $("#page-title").textContent = title;
  $("#breadcrumb").textContent = title;
  $("#page-description").textContent = description;
  $("#page-eyebrow").textContent =
    current === "home"
      ? "YOUR HOSTING WORKSPACE"
      : me.role === "admin"
        ? "SERVER ADMINISTRATION"
        : "YOUR WORKSPACE";
  const action = $("#page-action");
  action.hidden = ![
    "apps",
    "domains",
    "databases",
    "dns",
    "schedules",
    "backups",
    "users",
    "blocks",
  ].includes(current);
  action.innerHTML =
    (current === "apps"
      ? '<img class="size-5 object-contain" src="/assets/deploy.png" alt="">'
      : glyph("plus", "size-4")) +
    (current === "users"
      ? "Add user"
      : current === "blocks"
        ? "Block IP"
        : "Create");
  $("#content").innerHTML =
    '<div class="busy flex min-h-72 items-center justify-center text-xs text-slate-400">Loading your workspace…</div>';
  try {
    if (me.role === "admin" && !userList.length) userList = await api("/users");
    if (current === "home") await dashboard();
    else if (current === "security") security();
    else if (current === "audit") await auditPage();
    else if (current === "terminal" || current === "files")
      await workspace(current);
    else if (current === "settings") settings();
    else if (current === "docs" || current === "admin-api")
      await docs.render(current);
    else if (
      [
        "monitoring",
        "analytics",
        "integrations",
        "backup-center",
        "certificates",
        "egress",
        "jobs",
      ].includes(current)
    )
      await extra.render(current);
    else if (current === "users") await usersPage();
    else await resourcePage(current);
  } catch (e) {
    $("#content").innerHTML =
      `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">${esc(e.message)}</div>`;
  }
}
const tool = (page, title, subtitle, icon) =>
  `<a class="tool group flex items-center gap-3 rounded-lg p-3 text-left transition motion-reduce:transition-none hover:bg-[#f3f7f0] [&_strong]:block [&_strong]:text-[11px] [&_strong]:font-medium [&_small]:mt-1 [&_small]:block [&_small]:text-[10px] [&_small]:text-slate-400" href="#${page}"><span class="tool-icon flex size-10 shrink-0 items-center justify-center rounded-xl border border-slate-200/70 bg-white text-emerald-700/70 transition group-hover:border-emerald-200 group-hover:bg-white group-hover:text-emerald-700">${glyph(pages[page][2], "size-5")}</span><span><strong>${title}</strong><small>${subtitle}</small></span></a>`;
function toolGroup(title, items) {
  return `<section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><div class="card-head flex items-center justify-between gap-3 border-b border-slate-100 px-5 py-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_small]:text-[9px] [&_small]:font-medium [&_small]:tracking-wider [&_small]:text-slate-400"><h3>${title}</h3><small>${items.length} TOOLS</small></div><div class="tools-grid grid grid-cols-1 gap-1 p-3 sm:grid-cols-3">${items.map((i) => tool(...i)).join("")}</div></section>`;
}
async function dashboard() {
  const d = await api("/overview");
  const c = d.counts;
  const memory = d.host.memory || [];
  const total = parseInt((memory[0] || "").split(":")[1]) || 0;
  const available = parseInt((memory[2] || "").split(":")[1]) || 0;
  const used = total ? Math.round(((total - available) / total) * 100) : 0;
  $("#content").innerHTML =
    `<div class="stats mb-7 grid grid-cols-2 gap-3 xl:grid-cols-4 xl:gap-4">${[
      ["Applications", c.apps || 0, "Ready for your next idea", "boxes"],
      ["Domains", c.domains || 0, "Connected to your workspace", "globe"],
      [
        "Databases",
        c.databases || 0,
        "Isolated database identities",
        "database",
      ],
      ["Backups", c.backups || 0, "Application snapshots", "archive"],
    ]
      .map(
        ([label, n, sub, icon]) =>
          `<div class="stat group rounded-xl border border-slate-200/70 bg-white p-5 shadow-xs motion-safe:animate-enter transition motion-reduce:transition-none hover:border-emerald-200"><div class="stat-label flex items-center justify-between text-[11px] font-medium text-slate-400 [&_i]:rounded-lg [&_i]:bg-[#f1f5ee] [&_i]:p-2 [&_i]:text-emerald-700/70">${label}<i>${glyph(icon, "size-5")}</i></div><div class="stat-number mb-1 mt-3 text-[32px] font-medium leading-tight tracking-tight text-slate-800">${n}</div><small class="text-[10px] text-slate-400">${sub}</small></div>`,
      )
      .join(
        "",
      )}</div><div class="dashboard-grid grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_280px]"><div><div class="welcome-card relative mb-6 flex min-h-56 items-center overflow-hidden rounded-xl bg-forest p-6 text-white sm:p-7 [&_h2]:relative [&_h2]:z-10 [&_h2]:mb-3 [&_h2]:max-w-80 [&_h2]:text-[25px] [&_h2]:font-medium [&_h2]:tracking-tight [&_p]:relative [&_p]:z-10 [&_p]:text-xs [&_p]:leading-6 [&_p]:text-emerald-50/45 [&_.eyebrow]:text-mint [&_a]:relative [&_a]:z-10"><div><div class="eyebrow mb-2 text-[9px] font-semibold tracking-[.18em] text-slate-400">BUILD SOMETHING GREAT</div><h2>Welcome back, ${esc(me.username)}.</h2><p>Your next website, API, or bot starts here.<br>Give your ideas a place to grow.</p><a href="#apps" class="mt-5 inline-flex items-center gap-3 rounded-lg bg-mint px-4 py-2.5 text-xs font-medium text-forest transition motion-reduce:transition-none hover:bg-lime-200"><img class="size-6 object-contain" src="/assets/deploy.png" alt="">Deploy an application ${glyph("arrow-up-right", "size-4")}</a></div><img class="hidden sm:block absolute -right-5 top-0 size-52 object-contain opacity-90 sm:right-0 sm:size-60" src="/assets/deploy.png" alt=""></div><input id="tool-search" class="search-tools mb-6 w-full rounded-xl border border-slate-200/70 bg-white px-5 py-3.5 text-xs outline-none transition placeholder:text-slate-400 focus:border-emerald-400 focus:ring-4 focus:ring-emerald-50" placeholder="Search your tools — domains, databases, terminal…" aria-label="Find a tool">${toolGroup(
      "Applications & files",
      [
        ["apps", "Application manager", "Deploy websites & bots"],
        ["files", "File manager", "Browse your workspace"],
        ["terminal", "Terminal", "Packages & commands"],
        ["backup-center", "Full backups", "Files, SQL & remote storage"],
        ["schedules", "Scheduled tasks", "Automate your work"],
        ["apps", "Runtime catalog", "Python, PHP, Java & more"],
      ],
    )}${toolGroup("Domains & databases", [
      ["domains", "Domains", "Assign & connect"],
      ["dns", "DNS zone editor", "A, AAAA, MX, TXT & more"],
      ["certificates", "SSL / TLS & CDN", "Certificates & DNS export"],
      ["databases", "MySQL databases", "Create & manage"],
      ["databases", "PostgreSQL databases", "SQL for your apps"],
      ["databases", "Remote database access", "Control access by IP"],
    ])}${toolGroup("Monitoring & automation", [
      ["monitoring", "Website monitoring", "Uptime & Telegram alerts"],
      ["analytics", "Website analytics", "Visitors, heatmaps & SEO"],
      ["integrations", "Integrations", "Telegram, S3 & SSH"],
      ["egress", "Outgoing traffic", "Application SOCKS routing"],
      ["jobs", "Background jobs", "Progress & delivery results"],
      ["backups", "Legacy snapshots", "Workspace-only archives"],
    ])}${toolGroup("Security & administration", [
      ["security", "Security center", "View active protections"],
      [
        me.role === "admin" ? "blocks" : "settings",
        "Access controls",
        "IP rules & account access",
      ],
      [
        me.role === "admin" ? "users" : "settings",
        "Account manager",
        "Users & permissions",
      ],
      ["audit", "Activity log", "Review recent changes"],
      ["settings", "Account settings", "Password & session"],
      ["security", "Resource isolation", "Containers & permissions"],
    ])}</div><aside class="dashboard-aside grid gap-0 sm:grid-cols-2 sm:gap-5 xl:block"><section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Server information</h3><div class="server-meta flex items-center gap-3 border-b border-slate-100 pb-4 [&_b]:block [&_b]:text-xs [&_b]:font-medium [&_small]:text-[10px] [&_small]:text-slate-400"><span class="server-symbol flex size-10 items-center justify-center rounded-xl border border-slate-200 bg-slate-50 text-slate-500">${glyph("server", "size-5")}</span><div><b>CGPanel host</b><small>${d.host.agent === "online" ? "Agent connected" : "Tenant workspace"}</small></div></div><div class="detail-line mt-4 flex justify-between gap-2 text-[11px] text-slate-400 [&_strong]:font-medium [&_strong]:text-slate-600"><span>Account</span><strong>${me.role === "admin" ? "Administrator" : "Tenant"}</strong></div><div class="detail-line mt-4 flex justify-between gap-2 text-[11px] text-slate-400 [&_strong]:font-medium [&_strong]:text-slate-600"><span>Panel version</span><strong>${esc(d.version)} alpha</strong></div><div class="detail-line mt-4 flex justify-between gap-2 text-[11px] text-slate-400 [&_strong]:font-medium [&_strong]:text-slate-600"><span>Control plane</span><strong>Rust / Axum</strong></div>${total ? `<div class="meter mt-5"><div class="meter-label flex justify-between text-[11px] text-slate-400 [&_b]:font-medium [&_b]:text-emerald-700"><span>Host memory</span><b>${used}%</b></div><div class="meter-track mt-2 h-1.5 overflow-hidden rounded-full bg-slate-100"><div class="meter-fill h-full rounded-full bg-emerald-600" data-percent="${used}"></div></div></div><div class="detail-line mt-4 flex justify-between gap-2 text-[11px] text-slate-400 [&_strong]:font-medium [&_strong]:text-slate-600"><span>Load average</span><strong>${esc(String(d.host.load).split(" ").slice(0, 3).join(" / "))}</strong></div>` : ""}<div class="notice-line mt-5 border-t border-slate-100 pt-4 text-[10px] text-emerald-600">● ${d.host.agent === "online" ? "Host agent online" : "Account connected"}</div></section><section class="card mb-5 overflow-hidden rounded-xl border shadow-xs security-card relative overflow-hidden border-emerald-200/50 bg-[#edf4e8] p-5 [&_h3]:relative [&_h3]:max-w-40 [&_h3]:text-lg [&_h3]:font-medium [&_h3]:leading-snug [&_h3]:text-forest [&_p]:relative [&_p]:mb-5 [&_p]:mt-3 [&_p]:text-[11px] [&_p]:text-emerald-900/45 [&_a]:relative [&_a]:text-[11px] [&_a]:font-medium [&_a]:text-emerald-800"><img class="absolute -right-5 -top-5 size-32 object-contain opacity-90" src="/assets/security.png" alt=""><div class="eyebrow mb-2 text-[9px] font-semibold tracking-[.18em] text-slate-400 relative">SECURITY BY DESIGN</div><h3>Separate spaces.<br>Clear boundaries.</h3><p>Rootless application containers, tenant permissions, and a restricted provisioning service.</p><a href="#security"><span class="inline-flex items-center gap-2"><img class="size-6 object-contain" src="/assets/security.png" alt="">Explore your security controls ${glyph("arrow-up-right", "size-3.5")}</span></a></section><section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><div class="card-head flex items-center justify-between gap-3 border-b border-slate-100 px-5 py-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_small]:text-[9px] [&_small]:font-medium [&_small]:tracking-wider [&_small]:text-slate-400"><h3>Recent activity</h3><a href="#audit"><small>VIEW ALL ↗</small></a></div><div class="events px-5 py-1">${
      d.events
        .slice(0, 5)
        .map(
          (e) =>
            `<div class="event flex items-center gap-3 border-b border-slate-100 py-3.5 last:border-b-0 [&_b]:block [&_b]:text-[11px] [&_b]:font-medium [&_small]:text-[9px] [&_small]:text-slate-400"><span class="event-dot flex size-7 shrink-0 items-center justify-center rounded-full bg-emerald-50 text-emerald-600">${glyph("activity", "size-3")}</span><div><b>${esc(e.action.replaceAll("_", " "))}</b><small>${esc(e.actor)} · ${esc(e.created)} UTC</small></div></div>`,
        )
        .join("") ||
      '<p class="empty flex flex-col items-center px-5 py-16 text-center [&_h3]:mb-2 [&_h3]:text-lg [&_h3]:font-medium [&_p]:mb-6 [&_p]:text-sm [&_p]:text-slate-400">No activity yet.</p>'
    }</div></section></aside></div>`;
  $("#tool-search").oninput = (e) =>
    document
      .querySelectorAll(".tool")
      .forEach(
        (t) =>
          (t.hidden = !t.textContent
            .toLowerCase()
            .includes(e.target.value.toLowerCase())),
      );
  document
    .querySelectorAll("[data-percent]")
    .forEach((e) =>
      e.classList.add(
        [
          "w-0",
          "w-[10%]",
          "w-[20%]",
          "w-[30%]",
          "w-[40%]",
          "w-[50%]",
          "w-[60%]",
          "w-[70%]",
          "w-[80%]",
          "w-[90%]",
          "w-full",
        ][Math.min(10, Math.round(Number(e.dataset.percent) / 10))],
      ),
    );
}
function ownerName(id) {
  return (
    userList.find((u) => u.id === id)?.username ||
    (id === me.id ? me.username : id.slice(0, 10))
  );
}
function empty(kind) {
  return `<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs empty flex flex-col items-center px-5 py-16 text-center [&_h3]:mb-2 [&_h3]:text-lg [&_h3]:font-medium [&_p]:mb-6 [&_p]:text-sm [&_p]:text-slate-400"><div class="empty-icon mb-5 flex size-20 items-center justify-center rounded-3xl border border-emerald-100 bg-emerald-50 text-emerald-500/60">${glyph(pages[kind][2], "size-10")}</div><h3>Your ${pages[kind][0].toLowerCase()} start here</h3><p>Create your first item to get started.</p><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" data-create>＋ Create ${kind === "backups" ? "backup" : kind === "dns" ? "record" : kind.replace(/s$/, "")}</button></div>`;
}
async function resourcePage(kind) {
  const rows = await load(kind);
  const notices = {
    domains:
      "Administrators assign a domain to an account. Tenants can then create subdomains within their assigned domains. Point DNS to this server before requesting a certificate.",
    databases:
      "Credentials are shown once at creation. External database connections require an exact IP allowlist and TLS. For local applications, use an approved reachable address or an SSH tunnel; container localhost is separate from host localhost.",
    dns: "Changes update the local authoritative DNS service. Delegate your domain at its registrar to use this server’s nameserver, including glue records when needed.",
    apps: "Applications use 512 MB RAM, 1 CPU, and up to 128 processes each. Web applications listen on port 8080. Workers support polling Telegram bots and background jobs.",
    backups:
      "Backups contain application files. Database dumps, off-server copies, and automatic retention are not included. Restoring stops and restarts the linked application.",
    schedules:
      "Hourly, daily, and weekly jobs run inside the selected application container. The application must be running; each command is limited to 25 seconds.",
    blocks:
      "These rules block traffic from an exact IP address. Check carefully before blocking an address you use for administration.",
  };
  let html = notices[kind]
    ? `<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">${notices[kind]}</div>`
    : "";
  if (!rows.length) {
    $("#content").innerHTML = html + empty(kind);
    return;
  }
  const cols = {
    apps: ["Application", "Runtime / mode", "Owner", "Actions"],
    domains: ["Domain", "Application", "Owner", "Actions"],
    databases: ["Database", "Engine / port", "Allowed IPs", "Actions"],
    dns: ["Record", "Type / TTL", "Value", "Actions"],
    schedules: ["Task", "Schedule", "Application", "Actions"],
    backups: ["Snapshot", "Size", "Created", "Actions"],
    blocks: ["IP address", "Owner", "Created", "Actions"],
  }[kind];
  html += `<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs table-wrap overflow-x-auto"><table class="data-table w-full border-collapse whitespace-nowrap text-left [&_th]:bg-slate-50/70 [&_th]:px-5 [&_th]:py-3.5 [&_th]:text-[9px] [&_th]:font-medium [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-5 [&_td]:text-xs [&_td_small]:mt-1 [&_td_small]:block [&_td_small]:max-w-48 [&_td_small]:truncate [&_td_small]:text-[10px] [&_td_small]:text-slate-400 [&_tbody_tr]:transition [&_tbody_tr:hover]:bg-slate-50/50"><thead><tr>${cols.map((c) => `<th>${c}</th>`).join("")}</tr></thead><tbody>${rows
    .map((r) => {
      let d = r.data;
      let cells = [];
      let actions = [];
      if (kind === "apps") {
        cells = [
          `<b>${esc(r.name)}</b><small>${r.id.slice(0, 12)}</small>`,
          `<span class="pill inline-flex items-center rounded-md border border-emerald-100 bg-emerald-50/70 px-2 py-1 text-[10px] font-medium text-emerald-700 blue border-sky-100 bg-sky-50/70 text-sky-700">${esc(d.runtime)}</span> <span class="pill inline-flex items-center rounded-md border border-emerald-100 bg-emerald-50/70 px-2 py-1 text-[10px] font-medium text-emerald-700">${esc(d.mode)}</span>`,
          esc(ownerName(r.owner)),
        ];
        actions = [
          ["inspect", "Status"],
          ["start", "Start"],
          ["stop", "Stop"],
          ["restart", "Restart"],
          ["logs", "Logs"],
          ["open-terminal", "Terminal"],
        ];
      }
      if (kind === "domains") {
        cells = [
          `<b>${esc(r.name)}</b>`,
          esc(d.app_id?.slice(0, 12) || "Welcome page"),
          esc(ownerName(r.owner)),
        ];
        actions = [["tls", "Enable HTTPS"]];
      }
      if (kind === "databases") {
        cells = [
          `<b>${esc(r.name)}</b><small>${esc(d.database)}</small>`,
          `${esc(d.engine)} / ${esc(d.port)}`,
          esc((d.allowed_ips || []).join(", ") || "Local only"),
        ];
        actions = [["access", "Access rules"]];
      }
      if (kind === "dns") {
        cells = [
          `<b>${esc(r.name)}</b><small>${esc(d.zone)}</small>`,
          `${esc(d.type)} / ${esc(d.ttl)}s`,
          esc(d.value),
        ];
      }
      if (kind === "schedules") {
        cells = [
          esc(r.name),
          esc(d.schedule) + " · " + esc(d.timezone || "UTC"),
          esc(d.app_id?.slice(0, 12)),
        ];
        actions = [["status", "Run history"]];
      }
      if (kind === "backups") {
        cells = [
          esc(r.name || r.id.slice(0, 12)),
          formatSize(d.bytes),
          esc(r.created),
        ];
        actions = [["restore", "Restore"]];
      }
      if (kind === "blocks") {
        cells = [esc(r.name), esc(ownerName(r.owner)), esc(r.created)];
      }
      return `<tr>${cells.map((c) => `<td>${c}</td>`).join("")}<td><div class="table-actions flex items-center gap-1.5 [&_button]:px-2.5 [&_button]:py-2 [&_button]:text-[10px]">${actions.map(([a, label]) => `<button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" data-action="${a}" data-id="${r.id}">${glyph({ inspect: "activity", start: "play", stop: "square", restart: "refresh-cw", logs: "logs", "open-terminal": "terminal", tls: "lock-keyhole", access: "key-round", restore: "archive" }[a] || "arrow-up-right", "size-3.5")} ${label}</button>`).join("")}<button class="danger inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-lg border border-red-100 bg-red-50/60 px-3 py-2 text-xs text-red-500 transition motion-reduce:transition-none hover:bg-red-100" data-delete="${r.id}">${glyph("trash", "size-3.5")} Delete</button></div></td></tr>`;
    })
    .join("")}</tbody></table></div>`;
  $("#content").innerHTML = html;
}
function formatSize(n) {
  return !n
    ? "—"
    : n > 1048576
      ? (n / 1048576).toFixed(1) + " MB"
      : (n / 1024).toFixed(1) + " KB";
}
const input = (name, label, type = "text", help = "", value = "") =>
  `<label>${label}<input name="${name}" type="${type}" value="${esc(value)}" ${type === "password" ? 'autocomplete="new-password"' : ""} required>${help ? `<small>${help}</small>` : ""}</label>`;
const textarea = (name, label, help = "", value = "") =>
  `<label>${label}<textarea name="${name}" spellcheck="false">${esc(value)}</textarea>${help ? `<small>${help}</small>` : ""}</label>`;
const select = (name, label, options, help = "") =>
  `<label>${label}<select name="${name}">${options.map(([v, l]) => `<option value="${esc(v)}">${esc(l)}</option>`).join("")}</select>${help ? `<small>${help}</small>` : ""}</label>`;
function ownerField() {
  return me.role === "admin"
    ? select(
        "owner",
        "Account",
        userList.filter((u) => u.enabled).map((u) => [u.id, u.username]),
      )
    : "";
}
function modal(title, fields, submit, handler) {
  $("#dialog-title").textContent = title;
  $("#dialog-fields").innerHTML = fields;
  $("#dialog-error").textContent = "";
  $("#dialog-submit").textContent = submit;
  $("#dialog-submit").hidden = !handler;
  $("#cancel-dialog").textContent = handler ? "Cancel" : "Close";
  $("#dialog-form").onsubmit = async (e) => {
    e.preventDefault();
    if (!handler) return;
    const b = $("#dialog-submit");
    b.disabled = true;
    $("#dialog-error").textContent = "";
    try {
      await handler(Object.fromEntries(new FormData(e.target)));
    } catch (e) {
      $("#dialog-error").textContent = e.message;
    } finally {
      b.disabled = false;
    }
  };
  $("#dialog").showModal();
}
$("#close-dialog").onclick = $("#cancel-dialog").onclick = () => {
  if (!$("#dialog-submit").disabled) $("#dialog").close();
};
async function create(kind = current) {
  try {
    let fields = "";
    if (me.role === "admin") userList = await api("/users");
    if (["domains", "schedules", "backups"].includes(kind)) await load("apps");
    if (kind === "dns") await load("domains");
    const apps = () =>
      resources.apps.map((a) => [a.id, `${a.name} (${ownerName(a.owner)})`]);
    if (kind === "users") {
      fields =
        input(
          "username",
          "Username",
          "text",
          "Lowercase letters, numbers, and underscores.",
        ) +
        input("password", "Password", "password", "At least 14 characters.") +
        input(
          "quota",
          "Maximum resources per type",
          "number",
          "Applies to applications, domains, databases, and other resource types.",
          "10",
        ) +
        textarea(
          "allowed_ips",
          "Panel IP restrictions",
          "One CIDR per line; leave blank for unrestricted source addresses.",
        );
    } else {
      fields = ownerField();
      if (kind === "apps")
        fields +=
          input("name", "Application name") +
          select("runtime", "Runtime", [
            ["python", "Python 3.13"],
            ["php", "PHP 8.4"],
            ["node", "Node.js 22"],
            ["java", "Java 21"],
            ["rust", "Rust"],
            ["static", "Static website"],
          ]) +
          select("mode", "Workload", [
            ["web", "Website / API / webhook"],
            ["worker", "Background worker / Telegram polling bot"],
          ]) +
          textarea(
            "command",
            "Start command",
            "Leave empty for a working starter. Web apps must listen on 0.0.0.0:8080.",
          ) +
          textarea(
            "env",
            "Environment variables (JSON)",
            'Example: {"BOT_TOKEN":"your-token"}. Values are sent to the container and omitted from panel resource responses.',
            "{}",
          );
      if (kind === "domains")
        fields +=
          input(
            "name",
            "Domain name",
            "text",
            "Example: example.com or api.example.com",
          ) +
          select("app_id", "Connected application", [
            ["", "Welcome page"],
            ...apps(),
          ]);
      if (kind === "databases")
        fields +=
          input("name", "Database label") +
          select("engine", "Engine", [
            ["mysql", "MySQL compatible (MariaDB)"],
            ["postgresql", "PostgreSQL"],
          ]) +
          textarea(
            "allowed_ips",
            "External IP allowlist",
            "One exact IPv4 or IPv6 address per line. Blank means local only.",
          );
      if (kind === "dns")
        fields +=
          select(
            "domain_id",
            "Domain",
            resources.domains.map((d) => [
              d.id,
              `${d.name} (${ownerName(d.owner)})`,
            ]),
          ) +
          input(
            "name",
            "Record name",
            "text",
            "Use @ for the zone apex.",
            "@",
          ) +
          select(
            "type",
            "Record type",
            ["A", "AAAA", "CNAME", "MX", "TXT", "NS"].map((x) => [x, x]),
          ) +
          input("value", "Value", "text", "MX example: 10 mail.example.com") +
          input("ttl", "TTL (seconds)", "number", "", "300");
      if (kind === "schedules")
        fields +=
          input("name", "Task name") +
          select("app_id", "Application", apps()) +
          input(
            "schedule",
            "Crontab expression",
            "text",
            "Minute hour day-of-month month day-of-week. Supports *, ranges, lists, and steps.",
            "0 * * * *",
          ) +
          input(
            "timezone",
            "Timezone",
            "text",
            "IANA timezone, such as Asia/Tehran or UTC.",
            "UTC",
          ) +
          textarea(
            "command",
            "Command",
            "Runs as the unprivileged container user.",
          );
      if (kind === "backups")
        fields +=
          input("name", "Snapshot label") +
          select("app_id", "Application", apps());
      if (kind === "blocks") fields += input("name", "IP address");
    }
    modal(
      kind === "users"
        ? "Create tenant account"
        : `Create ${kind === "dns" ? "DNS record" : kind.replace(/s$/, "")}`,
      fields,
      "Create",
      async (v) => {
        if (v.allowed_ips !== undefined)
          v.allowed_ips = v.allowed_ips
            .split(/[\n,]+/)
            .map((s) => s.trim())
            .filter(Boolean);
        if (v.quota) v.quota = Number(v.quota);
        if (v.ttl) v.ttl = Number(v.ttl);
        if (v.env) {
          try {
            v.env = JSON.parse(v.env);
          } catch {
            throw Error("Environment variables must be valid JSON.");
          }
        }
        const result = await api(
          kind === "users" ? "/users" : "/resources/" + kind,
          "POST",
          v,
        );
        $("#dialog").close();
        if (kind === "users") userList = [];
        await render();
        if (result.result?.password)
          modal(
            "Save your database credentials",
            `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">This password is shown once. Store it before closing this window.</div><pre class="secret-result overflow-auto whitespace-pre-wrap break-all rounded-xl border border-slate-200 bg-slate-50 p-5 font-mono text-xs leading-7">${esc(JSON.stringify(result.result, null, 2))}</pre>`,
            "",
            null,
          );
        else toast("Created successfully.");
      },
    );
  } catch (e) {
    toast(e.message);
  }
}
$("#page-action").onclick = () => create();
document.addEventListener("click", async (e) => {
  const b = e.target.closest(
    "[data-create],[data-delete],[data-action],[data-user]",
  );
  if (!b) return;
  if (b.hasAttribute("data-create")) return create(b.dataset.create || current);
  if (b.dataset.delete) {
    modal(
      "Delete this resource?",
      `<p>This removes the resource from the server. Application workspaces are retained; database deletion is permanent.</p>`,
      "Delete",
      async () => {
        await api("/resource/" + b.dataset.delete, "DELETE");
        $("#dialog").close();
        await render();
        toast("Resource deleted.");
      },
    );
    return;
  }
  if (b.dataset.user) return editUser(b.dataset.user);
  if (b.dataset.action) {
    const action = b.dataset.action,
      id = b.dataset.id;
    if (action === "open-terminal") {
      sessionStorage.setItem("cg-app", id);
      location.hash = "terminal";
      return;
    }
    if (action === "tls") {
      await extra.tlsForm(id);
      return;
    }
    if (action === "access") {
      const r = resources.databases.find((r) => r.id === id);
      modal(
        "Database access rules",
        textarea(
          "allowed_ips",
          "Allowed external IPs",
          "One exact IP per line. Remote connections require TLS.",
          (r.data.allowed_ips || []).join("\n"),
        ),
        "Save rules",
        async (v) => {
          await api(`/resource/${id}/access`, "POST", {
            allowed_ips: v.allowed_ips
              .split(/[\n,]+/)
              .map((s) => s.trim())
              .filter(Boolean),
          });
          $("#dialog").close();
          await render();
        },
      );
      return;
    }
    if (action === "restore") {
      modal(
        "Restore application files",
        `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">The linked application will stop while files from this snapshot overwrite the workspace. Files added since the snapshot are retained.</div>` +
          input("confirm", "Type RESTORE to continue"),
        "Restore",
        async (v) => {
          await api(`/resource/${id}/restore`, "POST", v);
          $("#dialog").close();
          toast("Application files restored.");
        },
      );
      return;
    }
    b.disabled = true;
    try {
      const r = await api(`/resource/${id}/${action}`, "POST", {});
      if (action === "status") {
        const history = [...(r.history || [])].reverse();
        modal(
          "Scheduled task history",
          `<p class="mb-4 text-xs text-slate-500">Next run: ${r.next ? esc(new Date(r.next * 1000).toLocaleString()) : "Not scheduled"} · ${esc(r.timezone || "UTC")}</p>` +
            (history.length
              ? history
                  .map(
                    (entry) =>
                      `<section class="mb-3 rounded-xl border border-slate-200 p-4"><div class="mb-2 flex justify-between text-xs"><span>${esc(new Date(entry.started * 1000).toLocaleString())}</span><span class="${entry.success ? "text-emerald-700" : "text-red-600"}">${entry.success ? "Succeeded" : "Failed"}</span></div><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-slate-500">${esc(entry.output || "No output")}</pre></section>`,
                  )
                  .join("")
              : '<p class="text-xs text-slate-500">This task has not run yet.</p>'),
          "",
          null,
        );
      } else if (["logs", "inspect"].includes(action))
        modal(
          action === "logs" ? "Application logs" : "Application status",
          `<pre class="secret-result overflow-auto whitespace-pre-wrap break-all rounded-xl border border-slate-200 bg-slate-50 p-5 font-mono text-xs leading-7">${esc(r.output ?? JSON.stringify(r, null, 2))}</pre>`,
          "",
          null,
        );
      else toast(`${action} completed.`);
    } catch (e) {
      toast(e.message);
    } finally {
      b.disabled = false;
    }
  }
});
async function workspace(mode) {
  const apps = await load("apps");
  if (!apps.length) {
    $("#content").innerHTML =
      '<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">Create an application first to use its isolated workspace.</div>' +
      empty("apps");
    $("[data-create]").onclick = (e) => {
      e.stopPropagation();
      create("apps");
    };
    return;
  }
  $("#content").innerHTML =
    `<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">${mode === "terminal" ? "Commands run as UID 1000 inside your rootless application container. Use pip --user, npm, Maven/Gradle, or Cargo in /workspace. Host root access and system package installation are unavailable. Each command has a 25-second limit; this is a command console, not an interactive TTY." : "Files are read and written through the application container. Relative paths stay inside the container; edits are limited to 256 KiB per file. The application must be running."}</div><div class="toolbar mb-5 flex flex-wrap items-center gap-3 [&_input]:min-w-0 [&_input]:flex-1 [&_input]:rounded-lg [&_input]:border [&_input]:border-slate-200 [&_input]:bg-white [&_input]:px-4 [&_input]:py-2.5 [&_input]:text-xs [&_select]:rounded-lg [&_select]:border [&_select]:border-slate-200 [&_select]:bg-white [&_select]:px-4 [&_select]:py-2.5 [&_select]:text-xs"><select id="app-picker" aria-label="Application">${apps.map((a) => `<option value="${a.id}">${esc(a.name)} · ${esc(a.data.runtime)}</option>`).join("")}</select>${mode === "files" ? '<button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="list-files">List files</button><input id="file-path" placeholder="Relative path, e.g. main.py" aria-label="Relative file path"><button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="read-file">Open</button><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" id="write-file">Save</button>' : ""}</div>${mode === "terminal" ? '<div class="terminal mb-5 overflow-hidden rounded-xl border border-slate-700 bg-[#122820] text-emerald-100/80 [&_pre]:min-h-80 [&_pre]:max-h-[520px] [&_pre]:overflow-auto [&_pre]:whitespace-pre-wrap [&_pre]:break-words [&_pre]:p-6 [&_pre]:font-mono [&_pre]:text-xs [&_pre]:leading-7"><div class="terminal-bar flex justify-between gap-3 border-b border-white/10 px-5 py-3.5 text-[10px] text-emerald-100/40"><span>CGPanel command console</span><span>UNPRIVILEGED · /workspace</span></div><pre id="terminal-output">Choose an application and enter a command.\nTry: id, ls -la, python --version\n</pre><form class="command-line flex items-center gap-3 border-t border-white/10 p-4 text-mint [&_input]:min-w-0 [&_input]:flex-1 [&_input]:bg-transparent [&_input]:font-mono [&_input]:text-xs [&_input]:text-emerald-50 [&_input]:outline-none" id="command-form"><span>❯</span><input id="command" autocomplete="off" spellcheck="false" placeholder="Enter command…" aria-label="Command"><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50">Run ↵</button></form></div>' : '<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><textarea id="file-editor" class="editor min-h-96 w-full border-0 bg-[#122820] p-6 font-mono text-xs leading-7 text-emerald-100/85 outline-none" spellcheck="false" aria-label="File editor" placeholder="Open a file or enter a relative path and save a new file."></textarea></div><pre id="file-list" class="secret-result overflow-auto whitespace-pre-wrap break-all rounded-xl border border-slate-200 bg-slate-50 p-5 font-mono text-xs leading-7" hidden></pre>'}`;
  const picker = $("#app-picker");
  const saved = sessionStorage.getItem("cg-app");
  if (apps.some((a) => a.id === saved)) picker.value = saved;
  picker.onchange = () => sessionStorage.setItem("cg-app", picker.value);
  if (mode === "terminal")
    $("#command-form").onsubmit = async (e) => {
      e.preventDefault();
      const command = $("#command").value;
      if (!command.trim() || pending) return;
      pending = true;
      $("button", e.target).disabled = true;
      const out = $("#terminal-output");
      out.textContent += "\n❯ " + command + "\n";
      $("#command").value = "";
      try {
        const r = await api(`/resource/${picker.value}/terminal`, "POST", {
          command,
        });
        out.textContent += r.output;
      } catch (e) {
        out.textContent += e.message;
      } finally {
        pending = false;
        $("button", e.target).disabled = false;
        out.scrollTop = out.scrollHeight;
        $("#command").focus();
      }
    };
  else {
    const op = async (action, body) => {
      try {
        return await api(`/resource/${picker.value}/${action}`, "POST", body);
      } catch (e) {
        toast(e.message);
      }
    };
    $("#list-files").onclick = async () => {
      const r = await op("files", {});
      if (r) {
        $("#file-list").hidden = false;
        $("#file-list").textContent = r.output;
      }
    };
    $("#read-file").onclick = async () => {
      const r = await op("read", { path: $("#file-path").value });
      if (r) $("#file-editor").value = r.output;
    };
    $("#write-file").onclick = async () => {
      const r = await op("write", {
        path: $("#file-path").value,
        content: $("#file-editor").value,
      });
      if (r) toast("File saved.");
    };
  }
}
async function usersPage() {
  userList = await api("/users");
  $("#content").innerHTML =
    `<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">Tenants cannot become administrators, obtain host root, or access another tenant’s resources. Account suspension revokes panel sessions; running workloads remain online.</div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs table-wrap overflow-x-auto"><table class="data-table w-full border-collapse whitespace-nowrap text-left [&_th]:bg-slate-50/70 [&_th]:px-5 [&_th]:py-3.5 [&_th]:text-[9px] [&_th]:font-medium [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-5 [&_td]:text-xs [&_td_small]:mt-1 [&_td_small]:block [&_td_small]:max-w-48 [&_td_small]:truncate [&_td_small]:text-[10px] [&_td_small]:text-slate-400 [&_tbody_tr]:transition [&_tbody_tr:hover]:bg-slate-50/50"><thead><tr><th>Account</th><th>Role</th><th>State</th><th>Quota / type</th><th>Panel access</th><th>Actions</th></tr></thead><tbody>${userList.map((u) => `<tr><td><b>${esc(u.username)}</b><small>${u.id.slice(0, 12)}</small></td><td>${esc(u.role)}</td><td><span class="pill inline-flex items-center rounded-md border border-emerald-100 bg-emerald-50/70 px-2 py-1 text-[10px] font-medium text-emerald-700">${u.enabled ? "Enabled" : "Suspended"}</span></td><td>${u.quota}</td><td>${esc(u.allowed_ips.join(", ") || "Any source IP")}</td><td>${u.role === "admin" ? "—" : `<button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" data-user="${u.id}">Manage access</button>`}</td></tr>`).join("")}</tbody></table></div>`;
}
function editUser(id) {
  const u = userList.find((x) => x.id === id);
  modal(
    "Manage " + u.username,
    select("enabled", "Account state", [
      ["true", "Enabled"],
      ["false", "Suspended"],
    ]) +
      input("quota", "Maximum resources per type", "number", "", u.quota) +
      textarea(
        "allowed_ips",
        "Panel IP restrictions",
        "One CIDR per line. Existing sessions are revoked when you save.",
        u.allowed_ips.join("\n"),
      ) +
      textarea(
        "password",
        "New password (optional)",
        "Leave blank to keep the current password. At least 14 characters.",
      ),
    "Save",
    async (v) => {
      v.enabled = v.enabled === "true";
      v.quota = Number(v.quota);
      v.allowed_ips = v.allowed_ips
        .split(/[\n,]+/)
        .map((x) => x.trim())
        .filter(Boolean);
      await api("/users/" + id, "POST", v);
      $("#dialog").close();
      await render();
    },
  );
  $("[name=enabled]", $("#dialog")).value = u.enabled ? "true" : "false";
}
function security() {
  const features = [
    [
      "Tenant isolation",
      "Separate Linux identities and rootless Podman containers. No container has host root, added capabilities, or the host container socket.",
    ],
    [
      "Application limits",
      "Each application has a 512 MB memory cap, 1 CPU limit, and 128-process limit. A read-only root filesystem keeps package changes in its workspace.",
    ],
    [
      "Web traffic controls",
      "Nginx limits requests and concurrent connections. The firewall exposes hosting services and explicitly permitted database clients.",
    ],
    [
      "Account protection",
      "Argon2id password hashes, expiring HttpOnly sessions, CSRF checks, login throttling, and optional per-account CIDR filters.",
    ],
    [
      "Database boundaries",
      "Separate database credentials and permissions. Remote database traffic requires TLS and exact source-IP rules.",
    ],
    [
      "Administrative audit",
      "Provisioning operations and account changes are recorded. SSH brute-force controls are supplied by Fail2ban.",
    ],
  ];
  $("#content").innerHTML =
    `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">This community alpha has not undergone an independent security audit. Host controls cannot absorb a flood that saturates the server’s network link; arrange upstream DDoS protection with your provider. Disk quotas, WAF rules, MFA, and mail hosting are not implemented in this release.</div><div class="grid-two grid items-start gap-5 lg:grid-cols-2"><section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><div class="card-head flex items-center justify-between gap-3 border-b border-slate-100 px-5 py-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_small]:text-[9px] [&_small]:font-medium [&_small]:tracking-wider [&_small]:text-slate-400"><h3>Implemented protections</h3><small>CONFIGURATION SUMMARY</small></div>${features.map(([name, detail]) => `<div class="feature-item flex items-start gap-3 border-b border-slate-100 px-5 py-4 last:border-b-0 [&_strong]:text-xs [&_strong]:font-medium [&_p]:mt-1.5 [&_p]:text-[11px] [&_p]:leading-6 [&_p]:text-slate-400"><span class="feature-check mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-emerald-50 text-[10px] text-emerald-600">✓</span><div><strong>${name}</strong><p>${detail}</p></div></div>`).join("")}</section><section><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Security boundaries</h3><p>Tenant commands run inside their own containers. A separate Rust broker performs a fixed set of host operations over a local Unix socket.</p><p>The broker is trusted infrastructure. Keep it updated and restrict host SSH access to administrators.</p><a class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" href="#audit">Review activity →</a></div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Access management</h3><p>Apply source IP rules to tenant panel logins and to each database independently.</p><a class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" href="#${me.role === "admin" ? "users" : "settings"}">Manage accounts →</a></div></section></div>`;
}
async function auditPage() {
  const d = await api("/overview");
  $("#content").innerHTML =
    `<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs table-wrap overflow-x-auto"><table class="data-table w-full border-collapse whitespace-nowrap text-left [&_th]:bg-slate-50/70 [&_th]:px-5 [&_th]:py-3.5 [&_th]:text-[9px] [&_th]:font-medium [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-5 [&_td]:text-xs [&_td_small]:mt-1 [&_td_small]:block [&_td_small]:max-w-48 [&_td_small]:truncate [&_td_small]:text-[10px] [&_td_small]:text-slate-400 [&_tbody_tr]:transition [&_tbody_tr:hover]:bg-slate-50/50"><thead><tr><th>Action</th><th>Actor</th><th>Target</th><th>Time (UTC)</th></tr></thead><tbody>${d.events.map((e) => `<tr><td>${esc(e.action)}</td><td>${esc(e.actor)}</td><td>${esc(e.target)}</td><td>${esc(e.created)}</td></tr>`).join("")}</tbody></table></div><p>Showing the most recent 20 events available to your account.</p>`;
}
function settings() {
  $("#content").innerHTML =
    `<div class="grid-two grid items-start gap-5 lg:grid-cols-2"><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Your account</h3><p><b>${esc(me.username)}</b> · ${esc(me.role)}</p><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" id="change-password">Change password</button></div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Current session</h3><p>Sessions expire after eight hours. Password changes revoke all your sessions and API tokens.</p><button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="logout">Sign out</button></div></div>`;
  $("#logout").onclick = async () => {
    await api("/logout", "POST", {});
    showLogin();
  };
  $("#change-password").onclick = () =>
    modal(
      "Change password",
      input("current", "Current password", "password") +
        input(
          "password",
          "New password",
          "password",
          "Use at least 14 characters.",
        ),
      "Update password",
      async (v) => {
        await api("/password", "POST", v);
        $("#dialog").close();
        showLogin();
        toast("Password updated. Sign in again.");
      },
    );
}
const extra = features({
  api,
  esc,
  glyph,
  modal,
  input,
  textarea,
  select,
  toast,
  getMe: () => me,
  getCurrent: () => current,
});
const docs = documentation({
  api,
  esc,
  modal,
  input,
  select,
  textarea,
  toast,
  getMe: () => me,
  getCurrent: () => current,
});
boot();
