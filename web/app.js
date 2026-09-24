import { navIcon } from "/nav-icons.js";
import { mailPanel } from "/mail.js";
import { workspaceTools } from "/workspace.js";
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
let routeController, renderEpoch = 0;
let me,
  current = "home",
  userList = [],
  resources = {},
  csrf = "",
  pending = false;
const tools4 = workspaceTools({api,esc,glyph,toast,getCurrent:()=>current,getMe:()=>me});
const pages = {
  mail: ["Mail hosting", "Mailboxes, delivery security, and domain setup.", "mail"],
  services: ["Services", "Manage secure file-transfer access for each workspace.", "network"],
  "program-logs": ["Program logs", "Application output and errors, with automatic refresh.", "logs"],
  protection: ["Website protection", "Traffic limits, WAF, crawler policies, and request patterns.", "shield-check"],
  captcha: ["CAPTCHA", "Verify visitors locally or with your chosen provider.", "key-round"],
  editor: ["File editor", "Edit your workspace files with conflict protection.", "file-text"],
  ide: ["Workspace IDE", "Code, install extensions, and manage Git in your browser.", "folder-code"],
  runtimes: ["Runtime versions", "Choose the language version for each application.", "boxes"],
  updates: ["Panel updates", "Manage automatic updates from stable GitHub releases.", "settings"],
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
    "Browse, upload, organize, and edit your website files.",
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
  if (method === "GET" && routeController) options.signal = routeController.signal;
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
    if(!localStorage.getItem("cgp-tour-v5-"+me.id)) startTour();
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
    "ide",
    "services", "mail", "runtimes",
    "terminal",
    "program-logs",
    "schedules",
    "backup-center",
    "monitoring",
    "analytics",
    "certificates",
    "integrations",
    "egress",
    "jobs",
    "protection",
    "captcha",
    "security",
    ...(me.role === "admin" ? ["users", "blocks", "admin-api", "updates"] : []),
    "audit",
    "docs",
  ];
  $("#nav").innerHTML = keys
    .map(
      (k, i) =>
        `<a href="#${k}" data-nav="${k}" class="flex items-center gap-3 rounded-lg px-3 py-2.5 text-[12px] text-emerald-50/50 transition motion-reduce:transition-none hover:bg-white/5 hover:text-white [&.active]:bg-mint/10 [&.active]:text-mint ${i === 9 ? "nav-group mt-5 border-t border-white/10 pt-5" : ""}"><span class="nav-icon">${navIcon(k)}</span>${pages[k][0]}</a>`,
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
  const epoch = ++renderEpoch;
  routeController?.abort();
  routeController = new AbortController();
  current = location.hash.slice(1).split("?")[0] || "home";
  if (!pages[current]) current = "home";
  $(".sidebar").classList.remove("open");
  document
    .querySelectorAll("[data-nav]")
    .forEach((a) => a.classList.toggle("active", a.dataset.nav === current));
  const [title, description] = pages[current];
  document.querySelector('#app-help')?.remove();
  if(current==='apps'){const help=document.createElement('button');help.id='app-help';help.dataset.helpApp='';help.className='mt-3 rounded-lg border border-emerald-200 bg-white/60 px-3 py-2 text-xs text-emerald-700';help.textContent='What is an application?';$('#page-description').after(help)}
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
    if (epoch !== renderEpoch) return;
    if (current === "home") await dashboard();
    else if (["protection","captcha"].includes(current)) await protectionPage();
    else if (current === "security") security();
    else if (current === "audit") await auditPage();
    else if (["files","editor","ide","runtimes","updates"].includes(current)) await tools4.render(current);
    else if (current === "terminal") await workspace(current);
    else if (current === "program-logs") await programLogs();
    else if (current === "services") await servicesPage();
    else if (current === "mail") await mailPanel({api,esc,modal,input,select,getMe:()=>me,toast}).render();
    else if (current === "settings") await settings();
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
    if (epoch !== renderEpoch || e.name === "AbortError") return;
    $("#content").innerHTML =
      `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">${esc(e.message)}</div>`;
  }
}
function budgetSummary(budget){return '<div class="mb-5 grid grid-cols-3 gap-3">'+[['memory_mb','RAM','MiB',1],['cpu_millis','CPU','cores',1000],['disk_mb','Disk','MiB',1]].map(([key,label,unit,scale])=>'<div class="rounded-xl border border-emerald-100 bg-white/70 p-4"><p class="text-xs text-slate-500">'+label+' available</p><p class="mt-2 text-lg font-semibold">'+(budget.free[key]/scale)+' <span class="text-xs font-normal">'+unit+'</span></p><p class="mt-1 text-[10px] text-slate-400">'+(budget.allocated[key]/scale)+' / '+(budget.total[key]/scale)+' allocated</p></div>').join('')+'</div>'}
async function editBudget(uid){const budget=await api('/v5/users/'+uid+'/budget');modal('Account resource budget',budgetSummary(budget)+input('memory_mb','Total RAM (MiB)','number','Includes running IDE reservations.',budget.total.memory_mb)+input('cpu_millis','Total CPU (millicores)','number','1000 = one CPU core.',budget.total.cpu_millis)+input('disk_mb','Total workspace disk (MiB)','number',budget.disk_scope,budget.total.disk_mb),'Save budget',async value=>{await api('/v5/users/'+uid+'/budget','POST',Object.fromEntries(Object.entries(value).map(([k,v])=>[k,Number(v)])));$('#dialog').close();toast('Account resource budget saved.')})}
document.addEventListener('click',e=>{const button=e.target.closest('[data-budget]');if(button)editBudget(button.dataset.budget).catch(e=>toast(e.message))});
async function servicesPage(){
  const apps=await api('/resources/apps');const entries=await Promise.all(apps.map(async app=>({app,info:await api('/v5/apps/'+app.id+'/services')})));
  $('#content').innerHTML='<div class="grid gap-5 xl:grid-cols-2">'+entries.map(({app,info})=>'<section class="rounded-2xl border border-white/80 bg-white/75 p-6"><h2 class="text-lg font-semibold">'+esc(app.name)+'</h2><div class="my-5 grid grid-cols-2 gap-3"><div class="rounded-xl bg-emerald-50 p-4 text-sm">SFTP · '+(info.sftp?'Enabled':'Disabled')+'<p class="mt-2 text-xs text-slate-500">SSH · Port 22</p></div><div class="rounded-xl bg-emerald-50 p-4 text-sm">FTPS · '+(info.ftps?'Enabled':'Disabled')+'<p class="mt-2 text-xs text-slate-500">Explicit TLS · Port 21</p></div></div><dl class="space-y-2 text-xs text-slate-600"><div>Host: <code>'+esc(info.host)+'</code></div><div>Username: <code>'+esc(info.username)+'</code></div><div>Directory: <code>'+esc(info.directory)+'</code></div></dl><p class="my-5 text-xs leading-6 text-slate-500">'+esc(info.note)+' Use the server IP in your transfer client; ordinary Cloudflare proxying does not carry FTP or SSH.</p><div class="flex gap-3"><button data-service="'+app.id+'" class="rounded-xl bg-forest px-4 py-2.5 text-xs text-white">Manage access</button><button data-transfer-rotate="'+app.id+'" class="rounded-xl border border-slate-200 px-4 py-2.5 text-xs">Reset password</button></div></section>').join('')+'</div>';
  if(!apps.length)$('#content').textContent='Create an application to manage its file-transfer services.';
  const edit=(id,rotate)=>{const {app,info}=entries.find(e=>e.app.id===id);modal('Transfer access · '+app.name,
    '<label class="mb-4 flex items-center gap-3 text-sm"><input type="checkbox" name="sftp" '+(info.sftp?'checked':'')+'> Enable SFTP</label><label class="mb-4 flex items-center gap-3 text-sm"><input type="checkbox" name="ftps" '+(info.ftps?'checked':'')+'> Enable FTPS (explicit TLS)</label>'+textarea('allowed_ips','Allowed client IPs or prefixes','Blank allows any client address. Connections are confined to this application workspace.',(info.allowed_ips||[]).join('\n'))+'<p class="text-xs leading-6 text-slate-500">Changing access closes existing transfer sessions. '+(rotate?'This also generates a new password.':'A password is generated when this account is first created.')+'</p>',rotate?'Save and reset password':'Save access',async value=>{
      const result=await api('/v5/apps/'+id+'/services','POST',{sftp:!!value.sftp,ftps:!!value.ftps,rotate,allowed_ips:value.allowed_ips.split(/[\n,]+/).map(s=>s.trim()).filter(Boolean)});$('#dialog').close();await servicesPage();if(result.password)modal('Save your transfer password','<p class="mb-4 text-sm">Username: '+esc(result.username)+'</p><input readonly class="w-full rounded-xl border border-slate-200 p-3 font-mono text-xs" value="'+esc(result.password)+'"><p class="mt-4 text-xs text-slate-500">This password is shown once. It is separate from your panel password.</p>','',null);else toast('Transfer access saved.');
    });};
  document.querySelectorAll('[data-service]').forEach(b=>b.onclick=()=>edit(b.dataset.service,false));document.querySelectorAll('[data-transfer-rotate]').forEach(b=>b.onclick=()=>edit(b.dataset.transferRotate,true));
}
async function programLogs(){
  const apps=await api('/resources/apps');if(!apps.length){$('#content').textContent='Create an application to view its logs.';return}
  const saved=sessionStorage.getItem('cg-app');let chosen=apps.find(a=>a.id===saved)?.id||apps[0].id;
  $('#content').innerHTML='<section class="overflow-hidden rounded-2xl border border-white/70 bg-white/75"><header class="flex flex-wrap items-center gap-4 p-5"><select id="logs-app" class="rounded-lg border border-slate-200 p-2 text-xs">'+apps.map(a=>'<option value="'+a.id+'" '+(a.id===chosen?'selected':'')+'>'+esc(a.name)+' · '+esc(a.data.runtime)+'</option>').join('')+'</select><label class="flex items-center gap-2 text-xs"><input id="logs-live" type="checkbox" checked> Refresh every 3 seconds</label><button id="logs-refresh" class="rounded-lg border border-slate-200 px-3 py-2 text-xs">Refresh</button><span id="logs-state" class="text-xs text-slate-500"></span></header><pre id="program-output" class="h-[560px] overflow-auto whitespace-pre-wrap break-words bg-forest p-6 font-mono text-xs leading-6 text-emerald-50" aria-live="polite"></pre><p class="p-5 text-xs leading-6 text-slate-500">Last 200 lines from your application’s stdout and stderr. Configure your framework to log to stderr/stdout; errors written only to project log files are available in File manager. Secrets printed by your code also appear here.</p></section>';
  const output=$('#program-output');let active=false;const refresh=async()=>{if(active||current!=='program-logs'||!output.isConnected)return;active=true;const selected=chosen;try{const result=await api('/resource/'+chosen+'/logs','POST',{});if(!output.isConnected||selected!==chosen)return;const pinned=output.scrollHeight-output.scrollTop-output.clientHeight<40;output.textContent=result.output||'No application output yet.';if(pinned)output.scrollTop=output.scrollHeight;$('#logs-state').textContent='Updated '+new Date().toLocaleTimeString()}catch(e){if(output.isConnected)$('#logs-state').textContent=e.message}finally{active=false}};
  $('#logs-app').onchange=e=>{chosen=e.target.value;sessionStorage.setItem('cg-app',chosen);output.textContent='';refresh()};$('#logs-refresh').onclick=refresh;await refresh();
  const timer=setInterval(()=>{if(!output.isConnected){clearInterval(timer);return}if($('#logs-live').checked)refresh()},3000);
}
async function protectionPage(){
  const domains=await api('/resources/domains');
  if(!domains.length){$('#content').innerHTML='<p class="rounded-xl bg-white/70 p-8 text-sm text-slate-500">Add an assigned domain to configure website protection.</p>';return}
  const chosen=sessionStorage.getItem('cgp-protection-domain');const domain=domains.find(d=>d.id===chosen)||domains[0];
  const [config,traffic]=await Promise.all([api('/v5/protection/'+domain.id),api('/v5/protection/'+domain.id+'/traffic')]);
  const pick=(name,label,options)=>select(name,label,options.map(([value,title])=>[value,title]),config[name]);
  const providers=[['off','Disabled'],['local','Local arithmetic challenge'],['turnstile','Cloudflare Turnstile'],['hcaptcha','hCaptcha'],['recaptcha','Google reCAPTCHA v2 checkbox']];
  $('#content').innerHTML='<select id="protection-domain" class="mb-5 rounded-xl border border-slate-200 bg-white/80 p-3 text-sm">'+domains.map(d=>'<option value="'+d.id+'" '+(d.id===domain.id?'selected':'')+'>'+esc(d.name)+'</option>').join('')+'</select><div class="grid gap-6 xl:grid-cols-2"><section class="rounded-2xl border border-white/80 bg-white/75 p-6"><h2 class="mb-5 text-lg font-semibold">'+(current==='captcha'?'Visitor verification':'Request protection')+'</h2><form id="protection-form" class="space-y-4">'+
    (current==='captcha'?pick('captcha','Provider',providers)+input('site_key','Public site key','text','Not needed for the local challenge.',config.site_key||'')+input('secret','Provider secret','password',config.secret_set?'A secret is saved. Leave blank to retain it.':'Stored on the server, never sent to visitors.')+'<p class="rounded-lg bg-amber-50 p-4 text-xs leading-6 text-amber-900">When enabled, visitors must verify before accessing the website. API clients, webhooks, and crawlers cannot solve browser challenges. Leave this disabled for public APIs. Local arithmetic is a basic obstacle; advanced bots can solve it.</p>':
    input('requests_per_second','Requests per second per IP','number','Burst allowance: 40 requests.',config.requests_per_second)+input('connections','Concurrent connections per IP','number','',config.connections)+pick('waf','OWASP Core Rule Set WAF',[['off','Off'],['detect','Detect and log'],['enforce','Block matching requests']])+pick('crawlers','Crawler policy',[['allow','Allow crawlers'],['block_ai','Block known AI user agents'],['block_all','Block common crawler user agents']])+'<label class="flex items-center gap-3 text-sm"><input name="sitemap" type="checkbox" '+(config.sitemap?'checked':'')+'> Publish a sitemap</label>'+'<label class="flex items-center gap-3 text-sm"><input name="sitemap_auto" type="checkbox" '+(config.sitemap_auto?'checked':'')+'> Automatically include observed page paths</label><p class="text-xs text-slate-500">Automatic mapping uses up to 1,000 paths seen by your analytics tracker. Enable only if tracked pages are public; private route names would otherwise be published. Query strings are excluded.</p>'+textarea('sitemap_paths','Sitemap paths','One public path per line. Publishing replaces the panel-served /robots.txt and /sitemap.xml.',(config.sitemap_paths||['/']).join('\n'))+'<p class="text-xs leading-6 text-slate-500">Crawler rules match declared user agents and publish robots.txt. Bots can disguise themselves or ignore robots.txt. Start the WAF in detection mode and check your application before enabling blocking.</p>')+
    '<button class="rounded-xl bg-forest px-5 py-3 text-sm text-white">Save protection</button><p id="protection-result" class="text-xs text-emerald-700" role="status"></p></form></section><section class="rounded-2xl border border-white/80 bg-white/75 p-6"><h2 class="text-lg font-semibold">Recent traffic</h2><div class="my-5 grid grid-cols-3 gap-3">'+[['Requests',traffic.samples||0],['Rejected',traffic.rejected||0],['Server errors',traffic.server_errors||0]].map(([label,value])=>'<div class="rounded-xl bg-emerald-50 p-4"><p class="text-xs text-slate-500">'+label+'</p><p class="mt-2 text-2xl font-semibold">'+value+'</p></div>').join('')+'</div><p class="mb-5 text-xs leading-6 text-slate-500">'+esc(traffic.note)+'</p><div class="space-y-2">'+(traffic.sources||[]).map(r=>'<div class="flex flex-wrap justify-between gap-3 rounded-lg '+(r.unusual?'bg-amber-50':'bg-slate-50')+' p-3 text-xs"><span>'+esc(r.ip)+'</span><span>'+r.requests+' requests · '+r.rejected+' rejected'+(r.unusual?' · Investigate':'')+'</span></div>').join('')+'</div><p class="mt-6 text-xs leading-6 text-slate-500">These controls reduce application abuse. Attacks that saturate the network connection require upstream mitigation.</p></section></div>';
  // select() is shared with older forms; assign the actual stored selection explicitly.
  for(const key of ['captcha','waf','crawlers']){const field=$('#protection-form [name="'+key+'"]');if(field)field.value=config[key]}
  $('#protection-domain').onchange=e=>{sessionStorage.setItem('cgp-protection-domain',e.target.value);protectionPage().catch(e=>toast(e.message))};
  $('#protection-form').onsubmit=async e=>{e.preventDefault();const button=e.target.querySelector('button');button.disabled=true;try{const form=Object.fromEntries(new FormData(e.target));const next={...config,...form};delete next.secret_set;if(current==='protection'){next.requests_per_second=Number(form.requests_per_second);next.connections=Number(form.connections);next.sitemap=!!form.sitemap;next.sitemap_auto=!!form.sitemap_auto;next.sitemap_paths=(form.sitemap_paths||'').split('\n').map(s=>s.trim()).filter(Boolean)}await api('/v5/protection/'+domain.id,'POST',next);$('#protection-result').textContent='Protection saved. Existing visitor passes have been revoked.'}catch(error){toast(error.message)}finally{button.disabled=false}};
}
function startTour(){
  document.querySelector('#cgp-tour')?.remove();
  const steps=[['home','Your overview','Start here to see your applications, domains, and available tools.'],['apps','What is an application?','An application is an isolated workspace with files, a language runtime, and a startup command. A website, API, Telegram bot, or background worker each runs as an application. Domains connect visitors to a web application.'],['domains','Connect a domain','Add an assigned domain and choose the web application it should serve. Point your DNS records at this server or configure your CDN.'],['files','Manage your files','Browse folders, upload your website, and open text files in a separate editor tab. Deleted files move to Trash.'],['ide','Your development workspace','Open code-server to edit your project, use Git, install editor extensions, and run interactive commands.'],['schedules','Automate tasks','Create scheduled commands. The timing helper previews upcoming runs; Run command now executes your command immediately.'],['backup-center','Protect your work','Create full backups and keep an off-site copy. Review the archive and destination before restoring.'],['monitoring','Keep an eye on your site','Check availability and connect Telegram alerts. Analytics needs the tracker installed on your website.']];
  const guidance={databases:'Create a MySQL or PostgreSQL database, copy its credentials into your application, and restrict external access to the IPs or networks that need it.',dns:'Manage DNS records when this server is authoritative. If Cloudflare hosts your DNS, apply records there or export the zone.',services:'Enable SFTP or encrypted FTPS for an application, set allowed source networks, and rotate its transfer password.',mail:'Create domain mailboxes and manage passwords and quotas. Your administrator must configure the mail hostname, trusted certificate and public DNS first.',runtimes:'Choose a runtime track or an available official image tag. Switching briefly restarts the application; test code and dependencies before discarding the previous runtime.',terminal:'Run application commands with live output. Tab completes installed commands, arrow keys recall history, and clear resets the display. Use the IDE for interactive programs.','program-logs':'Select an application to watch its stdout and stderr. Configure your framework to write errors there so hidden failures appear here.',analytics:'Install the generated tracker to see page views, estimated unique IPs and click hotspots. Privacy settings can reduce what is collected.',certificates:'Issue and renew HTTPS certificates, choose HTTP or DNS verification, and export records for your CDN provider.',integrations:'Connect backup storage, Telegram alerts, DNS providers or SOCKS5 proxies. Secrets stay private and can be replaced later.',egress:'Route an application through a SOCKS5 gateway. Applying the change restarts the application; administrators can lock its proxy policy.',jobs:'Check queued and running operations here. A queued backup, certificate or IDE installation is not finished until its job succeeds.',protection:'Start the WAF in detection mode, inspect traffic and then enable blocking. Set per-IP request limits and optional crawler or sitemap policies.',captcha:'Choose local arithmetic verification, Cloudflare Turnstile, hCaptcha or reCAPTCHA. Site-wide verification can affect API clients and webhooks.',security:'Review the available isolation, access and traffic protections, and understand their operating limits.',users:'Create accounts, assign permitted domains and set RAM, CPU and workspace disk budgets. Disabling an account revokes its access.',blocks:'Block an individual IP or CIDR range. Read the displayed first and last address to avoid blocking your own management network.','admin-api':'Create scoped expiring administrator tokens and explore endpoint examples. Store each token privately; it is shown only once.',updates:'Control automatic stable-release updates, check for a release or apply one manually. Background jobs must finish before an update starts.',audit:'Review account and provisioning changes to investigate unexpected activity.',docs:'Read the bundled user guides, mail setup instructions and administrator API reference.'};
  for(const link of document.querySelectorAll('[data-nav]')){const key=link.dataset.nav;if(!steps.some(s=>s[0]===key)&&pages[key])steps.push([key,pages[key][0],guidance[key]||pages[key][1]]);}
  steps.push(['settings','Your account','Open the account menu to change your password, configure authenticator MFA, save recovery codes, or sign out.']);
  let step=0,highlight=null;const card=document.createElement('section');card.id='cgp-tour';card.setAttribute('role','dialog');card.setAttribute('aria-label','Workspace guide');card.className='fixed bottom-5 left-5 right-5 z-50 rounded-2xl border border-white/80 bg-white/95 p-6 shadow-2xl backdrop-blur-xl motion-safe:animate-enter md:bottom-auto md:left-72 md:right-auto md:top-24 md:w-96';document.body.append(card);
  const end=()=>{highlight?.classList.remove('ring-2','ring-emerald-400');localStorage.setItem('cgp-tour-v5-'+me.id,'done');card.remove();document.removeEventListener('keydown',keys)};
  const draw=()=>{highlight?.classList.remove('ring-2','ring-emerald-400');const [page,title,description]=steps[step];highlight=page==='settings'?document.querySelector('#account-menu'):document.querySelector('[data-nav="'+page+'"]');highlight?.classList.add('ring-2','ring-emerald-400');highlight?.scrollIntoView({block:'nearest'});card.innerHTML='<p class="text-xs font-medium text-emerald-700">WORKSPACE GUIDE · '+(step+1)+' / '+steps.length+'</p><h2 class="mt-3 text-lg font-semibold">'+esc(title)+'</h2><p class="mt-3 text-sm leading-6 text-slate-600">'+esc(description)+'</p><div class="mt-5 flex items-center gap-3"><button data-tour="skip" class="mr-auto text-xs text-slate-500">Close guide</button><button data-tour="back" class="rounded-lg border border-slate-200 px-3 py-2 text-xs" '+(step?'':'disabled')+'>Back</button><button data-tour="next" class="rounded-lg bg-forest px-4 py-2 text-xs text-white">'+(step===steps.length-1?'Finish':'Next')+'</button></div>';card.querySelector('[data-tour="skip"]').onclick=end;card.querySelector('[data-tour="back"]').onclick=()=>{step--;draw()};card.querySelector('[data-tour="next"]').onclick=()=>{if(++step===steps.length)end();else draw()};card.querySelector('[data-tour="next"]').focus()};
  const keys=e=>{if(e.key==='Escape'){end();return}if(e.key==='Tab'){const buttons=[...card.querySelectorAll('button:not(:disabled)')];if(e.shiftKey&&document.activeElement===buttons[0]){e.preventDefault();buttons.at(-1).focus()}else if(!e.shiftKey&&document.activeElement===buttons.at(-1)){e.preventDefault();buttons[0].focus()}}};document.addEventListener('keydown',keys);draw();
}
document.addEventListener('click',e=>{if(e.target.closest('[data-help-tour]'))startTour();if(e.target.closest('[data-help-app]'))modal('What is an application?','<p class="text-sm leading-7 text-slate-600">An application is your project: its files, language runtime, environment variables, and startup command. It runs in an isolated container without host root access. Use a web application for a website or API, or a worker for a Telegram bot and background processing. A domain directs visitors to a web application. Databases are managed separately and connected using credentials in your application configuration.</p>','',null)});
const tool = (page, title, subtitle, icon) =>
  `<a class="tool group flex items-center gap-3 rounded-lg p-3 text-left transition motion-reduce:transition-none hover:bg-[#f3f7f0] [&_strong]:block [&_strong]:text-[11px] [&_strong]:font-medium [&_small]:mt-1 [&_small]:block [&_small]:text-[10px] [&_small]:text-slate-400" href="#${page}"><span class="tool-icon flex size-10 shrink-0 items-center justify-center rounded-xl border border-slate-200/70 bg-white text-emerald-700/70 transition group-hover:border-emerald-200 group-hover:bg-white group-hover:text-emerald-700">${glyph(pages[page][2], "size-5")}</span><span><strong>${title}</strong><small>${subtitle}</small></span></a>`;
function toolGroup(title, items) {
  return `<section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><div class="card-head flex items-center justify-between gap-3 border-b border-slate-100 px-5 py-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_small]:text-[9px] [&_small]:font-medium [&_small]:tracking-wider [&_small]:text-slate-400"><h3>${title}</h3><small>${items.length} TOOLS</small></div><div class="tools-grid grid grid-cols-1 gap-1 p-3 sm:grid-cols-3">${items.map((i) => tool(...i)).join("")}</div></section>`;
}
async function dashboard() {
  const budget=await api("/v5/users/"+me.id+"/budget");
  const d = await api("/overview");
  if (current !== "home") return;
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
  $("#content").insertAdjacentHTML("afterbegin",budgetSummary(budget));
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
      "These rules block individual IPs or CIDR prefixes. Check the selected range carefully before blocking a network you use for administration.",
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
          ["limits", "Resources"],
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
        actions = [["status", "Run history"], ["test-cron", "Run now"]];
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
  const schedule=$('[name="schedule"]',$('#dialog-fields'));
    if(schedule){
      const helper=document.createElement('div');helper.className='mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 p-4 text-xs';
      helper.innerHTML='<label class="block">Quick schedule<select id="cron-preset" class="my-2 w-full rounded-lg border border-slate-200 bg-white p-2"><option value="">Custom expression</option><option value="*/5 * * * *">Every 5 minutes</option><option value="0 * * * *">Every hour</option><option value="0 0 * * *">Every day at midnight</option><option value="0 9 * * 1">Every Monday at 09:00</option><option value="0 0 1 * *">First day of each month</option></select></label><p class="text-slate-500">Fields: minute · hour · day of month · month · weekday. Times use the selected timezone.</p><button type="button" id="cron-preview" class="mt-3 rounded-lg bg-forest px-3 py-2 text-white">Show next 5 runs</button><ol id="cron-dates" class="mt-3 space-y-1"></ol><button type="button" id="cron-test" class="mt-3 rounded-lg border border-slate-300 px-3 py-2">Run command now</button><p class="mt-2 text-slate-500">Runs immediately and can change files or data, just like a scheduled run. Limited to 25 seconds.</p><pre id="cron-output" class="mt-3 max-h-60 overflow-auto whitespace-pre-wrap"></pre>';
      schedule.closest('label').after(helper);
      $('#cron-preset').onchange=e=>{if(e.target.value)schedule.value=e.target.value};
      $('#cron-preview').onclick=async()=>{try{const tz=$('[name="timezone"]').value;const result=await api('/v2/cron-preview','POST',{schedule:schedule.value,timezone:tz});$('#cron-dates').textContent=result.next.map(t=>new Date(t*1000).toLocaleString(undefined,{timeZone:tz})+' '+tz).join(' · ')}catch(e){$('#cron-dates').textContent=e.message}};
      $('#cron-test').onclick=async e=>{const button=e.currentTarget;button.disabled=true;try{const app=$('[name="app_id"]').value,command=$('[name="command"]').value;if(!command.trim())throw new Error('Enter a command first.');const result=await api('/resource/'+app+'/terminal','POST',{command});$('#cron-output').textContent=result.output||'(No output)'}catch(e){$('#cron-output').textContent=e.message}finally{button.disabled=false}};
    }
    document.querySelectorAll('#dialog-fields [name="allowed_ips"], #dialog-fields [data-ip-range]').forEach(field=>{
      const hint=document.createElement('p');hint.className='my-2 text-xs leading-6 text-slate-500';hint.setAttribute('aria-live','polite');field.after(hint);field.placeholder='10.10.10.0/24, 2001:db8::1';let timer;
      field.addEventListener('input',()=>{clearTimeout(timer);timer=setTimeout(async()=>{const value=field.value;try{const rows=await api('/v5/network-range','POST',{value});if(field.value===value)hint.textContent=rows.map(r=>r.error?r.input+': '+r.error:'From '+r.first+' till '+r.last).join(' · ')}catch(e){hint.textContent=e.message}},250)});
      hint.textContent='Enter individual IPs or CIDR prefixes.'+(current==='databases'?' MariaDB supports IPv4 prefixes and individual IPv6 addresses; PostgreSQL supports both.':'');
    });
    if($('#create-budget')){const owner=$('#dialog-fields [name="owner"]');const update=async()=>{const id=owner?.value||me.id;try{const budget=await api('/v5/users/'+id+'/budget');if($('#create-budget'))$('#create-budget').innerHTML=budgetSummary(budget)}catch(e){if($('#create-budget'))$('#create-budget').textContent=e.message}};owner?.addEventListener('change',update);update();}
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
          input("version", "Runtime version (optional)", "text", "Leave blank for the starter version, or use a supported track / official tag such as tag:3.14-slim-bookworm for Python. PHP custom tags must use FPM. Availability depends on the official image registry.").replace(" required", "") +
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
      if(kind==='apps') fields+='<div id="create-budget" class="mt-4"></div>'+input('memory_mb','RAM (MiB)','number','','512')+input('cpu_millis','CPU (millicores)','number','1000 = one CPU core.','1000')+input('disk_mb','Workspace disk (MiB)','number','Kernel-enforced volume capacity; filesystem metadata uses some space.','1024');
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
            "One IP address or CIDR prefix per line. Blank means local only.",
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
      if (kind === "blocks") fields += input("name", "IP address or CIDR prefix").replace("<input ","<input data-ip-range ");
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
        for(const key of ["memory_mb","cpu_millis","disk_mb"])if(v[key]!==undefined)v[key]=Number(v[key]);
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
    if(action==='limits'){const item=resources.apps.find(a=>a.id===id);const budget=await api('/v5/users/'+item.owner+'/budget');const entry=budget.applications.find(a=>a.id===id);modal('Application resources · '+item.name,
      budgetSummary(budget)+input('memory_mb','RAM (MiB)','number','',entry.limits.memory_mb)+input('cpu_millis','CPU (millicores)','number','1000 = one CPU core.',entry.limits.cpu_millis)+input('disk_mb','Workspace disk (MiB)','number','Volumes can grow in place; shrinking requires migration to a new application.',entry.limits.disk_mb)+'<p class="text-xs leading-6 text-slate-500">Applying limits briefly stops this application and its IDE. First-time disk enforcement migrates the workspace and retains a recovery copy. '+(entry.disk_enforced?'Disk enforcement is active.':'This existing workspace does not yet have an enforced disk limit.')+'</p>','Apply limits',async v=>{await api('/v5/apps/'+id+'/limits','POST',Object.fromEntries(Object.entries(v).map(([k,v])=>[k,Number(v)])));$('#dialog').close();await render();toast('Application limits applied.')});return;}
    if(action==='logs'){sessionStorage.setItem('cg-app',id);location.hash='program-logs';return;}
    if (action === "test-cron") {
      const task=resources.schedules.find(r=>r.id===id);
      modal('Run scheduled command now',`<p class="mb-4 text-sm">This runs immediately in the application and can change files or data. Execution is limited to 25 seconds.</p><pre class="mb-4 whitespace-pre-wrap rounded-xl bg-mint/20 p-4 text-xs">${esc(task.data.command)}</pre><pre id="cron-run-result" class="max-h-80 overflow-auto whitespace-pre-wrap text-xs"></pre>`,'Run command',async()=>{
        const result=await api('/resource/'+task.data.app_id+'/terminal','POST',{command:task.data.command});
        $('#cron-run-result').textContent=result.output||'Command completed without output.';
      });return;
    }
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
          "One IP or CIDR prefix per line. Remote connections require TLS.",
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
    `<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">${mode === "terminal" ? "Commands run as UID 1000 inside your rootless application container. Use pip --user, npm, Maven/Gradle, or Cargo in /workspace. Host root access and system package installation are unavailable. Each command has a 25-second limit; output streams live. Use Tab to complete installed commands, ↑/↓ for history, and clear or Ctrl+L to clear the display. Interactive programs can run in the Workspace IDE terminal." : "Files are read and written through the application container. Relative paths stay inside the container; edits are limited to 256 KiB per file. The application must be running."}</div><div class="toolbar mb-5 flex flex-wrap items-center gap-3 [&_input]:min-w-0 [&_input]:flex-1 [&_input]:rounded-lg [&_input]:border [&_input]:border-slate-200 [&_input]:bg-white [&_input]:px-4 [&_input]:py-2.5 [&_input]:text-xs [&_select]:rounded-lg [&_select]:border [&_select]:border-slate-200 [&_select]:bg-white [&_select]:px-4 [&_select]:py-2.5 [&_select]:text-xs"><select id="app-picker" aria-label="Application">${apps.map((a) => `<option value="${a.id}">${esc(a.name)} · ${esc(a.data.runtime)}</option>`).join("")}</select>${mode === "files" ? '<button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="list-files">List files</button><input id="file-path" placeholder="Relative path, e.g. main.py" aria-label="Relative file path"><button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="read-file">Open</button><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" id="write-file">Save</button>' : ""}</div>${mode === "terminal" ? '<div class="terminal mb-5 overflow-hidden rounded-xl border border-slate-700 bg-[#122820] text-emerald-100/80 [&_pre]:min-h-80 [&_pre]:max-h-[520px] [&_pre]:overflow-auto [&_pre]:whitespace-pre-wrap [&_pre]:break-words [&_pre]:p-6 [&_pre]:font-mono [&_pre]:text-xs [&_pre]:leading-7"><div class="terminal-bar flex justify-between gap-3 border-b border-white/10 px-5 py-3.5 text-[10px] text-emerald-100/40"><span>CGPanel command console</span><span>UNPRIVILEGED · /workspace</span></div><pre id="terminal-output">Choose an application and enter a command.\nTry: id, ls -la, python --version\n</pre><form class="command-line flex items-center gap-3 border-t border-white/10 p-4 text-mint [&_input]:min-w-0 [&_input]:flex-1 [&_input]:bg-transparent [&_input]:font-mono [&_input]:text-xs [&_input]:text-emerald-50 [&_input]:outline-none" id="command-form"><span>❯</span><input id="command" autocomplete="off" spellcheck="false" placeholder="Enter command…" aria-label="Command"><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50">Run ↵</button></form></div>' : '<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><textarea id="file-editor" class="editor min-h-96 w-full border-0 bg-[#122820] p-6 font-mono text-xs leading-7 text-emerald-100/85 outline-none" spellcheck="false" aria-label="File editor" placeholder="Open a file or enter a relative path and save a new file."></textarea></div><pre id="file-list" class="secret-result overflow-auto whitespace-pre-wrap break-all rounded-xl border border-slate-200 bg-slate-50 p-5 font-mono text-xs leading-7" hidden></pre>'}`;
  const picker = $("#app-picker");
  const saved = sessionStorage.getItem("cg-app");
  if (apps.some((a) => a.id === saved)) picker.value = saved;
  picker.onchange = () => sessionStorage.setItem("cg-app", picker.value);
  if(mode==='terminal'){
    const field=$('#command');let commands=['cd','clear','echo','exit','export','pwd'],history=[],historyAt=0;
    const hint=document.createElement('span');hint.className='pointer-events-none absolute inset-0 overflow-hidden whitespace-pre font-mono text-xs text-slate-500';hint.setAttribute('aria-hidden','true');
    const wrap=document.createElement('div');wrap.className='relative min-w-0 flex-1';field.replaceWith(wrap);wrap.append(hint,field);field.classList.add('relative','w-full');
    const suggestion=()=>{const value=field.value;return value&&!/\s/.test(value)?commands.find(x=>x.startsWith(value)&&x!==value)||'':''};
    field.addEventListener('input',()=>{hint.textContent=suggestion()});
    field.addEventListener('keydown',e=>{if(e.key==='Tab'){e.preventDefault();const next=suggestion();if(next)field.value=next+' ';hint.textContent=''}
      if(e.ctrlKey&&e.key.toLowerCase()==='l'){e.preventDefault();$('#terminal-output').textContent=''}
      if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();historyAt=Math.max(0,Math.min(history.length,historyAt+(e.key==='ArrowUp'?-1:1)));field.value=history[historyAt]||'';hint.textContent=''}
      if(e.key==='Enter'&&field.value.trim()){history.push(field.value);historyAt=history.length;hint.textContent=''}
    });
    const complete=async()=>{const chosen=picker.value;try{const result=await api(`/resource/${chosen}/terminal`,'POST',{command:'for d in /usr/local/bin /usr/bin /bin /workspace/.local/bin /workspace/.cargo/bin; do for f in "$d"/*; do [ -f "$f" ] && [ -x "$f" ] && printf "%s\\n" "${f##*/}"; done; done | head -2000'});if(picker.value===chosen)commands=[...new Set(['cd','clear','echo','export','pwd',...result.output.split(/\r?\n/).filter(x=>/^[a-zA-Z0-9_.+-]+$/.test(x))])].sort()}catch{}};
    picker.addEventListener('change',()=>{commands=['cd','clear','echo','pwd'];complete()});complete();
  }
  if (mode === "terminal")
    $("#command-form").onsubmit = async (e) => {
      e.preventDefault();
      const command = $("#command").value;
      if (!command.trim() || pending) return;
      if (command.trim() === "clear") { $("#terminal-output").textContent = ""; $("#command").value = ""; return; }
      pending = true;
      $("button", e.target).disabled = true;
      const out = $("#terminal-output");
      out.textContent += "\n❯ " + command + "\n";
      $("#command").value = "";
      try {
        const response=await fetch(`/api/v5/apps/${picker.value}/console`,{
          method:'POST',credentials:'same-origin',signal:routeController.signal,
          headers:{'Content-Type':'application/json','x-csrf-token':csrf},body:JSON.stringify({command})
        });
        if(!response.ok)throw new Error((await response.json()).error||'Command failed');
        const reader=response.body.getReader(),decoder=new TextDecoder(),outputDecoder={stdout:new TextDecoder(),stderr:new TextDecoder()};let buffer='';
        while(true){const {value,done}=await reader.read();if(done)break;buffer+=decoder.decode(value,{stream:true});
          let at;while((at=buffer.indexOf('\n'))>=0){const event=JSON.parse(buffer.slice(0,at));buffer=buffer.slice(at+1);
            if(event.type==='stdout'||event.type==='stderr')out.textContent+=outputDecoder[event.type].decode(Uint8Array.from(atob(event.data),c=>c.charCodeAt(0)),{stream:true});
            else if(event.type==='exit')out.textContent+='\n[exit '+event.code+']\n';
            else if(event.type==='error')out.textContent+='\n'+event.message+'\n';
            out.scrollTop=out.scrollHeight;
          }
        }
        out.textContent+=outputDecoder.stdout.decode()+outputDecoder.stderr.decode();
      } catch (e) {
        out.textContent += e.message;
      } finally {
        pending = false;
        $("button", e.target).disabled = false;
        out.scrollTop = out.scrollHeight;
        $("#command")?.focus();
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
    `<div class="info-box mb-5 rounded-xl border border-emerald-100 bg-emerald-50/60 px-5 py-4 text-xs leading-6 text-emerald-900/65">Tenants cannot become administrators, obtain host root, or access another tenant’s resources. Account suspension revokes panel sessions; running workloads remain online.</div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs table-wrap overflow-x-auto"><table class="data-table w-full border-collapse whitespace-nowrap text-left [&_th]:bg-slate-50/70 [&_th]:px-5 [&_th]:py-3.5 [&_th]:text-[9px] [&_th]:font-medium [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-5 [&_td]:text-xs [&_td_small]:mt-1 [&_td_small]:block [&_td_small]:max-w-48 [&_td_small]:truncate [&_td_small]:text-[10px] [&_td_small]:text-slate-400 [&_tbody_tr]:transition [&_tbody_tr:hover]:bg-slate-50/50"><thead><tr><th>Account</th><th>Role</th><th>State</th><th>Quota / type</th><th>Panel access</th><th>Actions</th></tr></thead><tbody>${userList.map((u) => `<tr><td><b>${esc(u.username)}</b><small>${u.id.slice(0, 12)}</small></td><td>${esc(u.role)}</td><td><span class="pill inline-flex items-center rounded-md border border-emerald-100 bg-emerald-50/70 px-2 py-1 text-[10px] font-medium text-emerald-700">${u.enabled ? "Enabled" : "Suspended"}</span></td><td>${u.quota}</td><td>${esc(u.allowed_ips.join(", ") || "Any source IP")}</td><td>${u.role === "admin" ? "—" : `<button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" data-user="${u.id}">Manage access</button> <button data-budget="${u.id}" class="rounded-lg border border-slate-200 px-3 py-2.5 text-xs">Resource budget</button>`}</td></tr>`).join("")}</tbody></table></div>`;
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
      "Admin account budgets govern application RAM, CPU and workspace disk allocations. Enforced workspace volumes prevent writes beyond their capacity; legacy workspaces can be migrated in Applications.",
    ],
    [
      "Web traffic controls",
      "Configure request limits, connection limits and OWASP CRS detection or blocking in Website protection. Review rejected requests before tightening policies.",
    ],
    [
      "Account protection",
      "Authenticator MFA and one-use recovery codes, Argon2id passwords, expiring HttpOnly sessions, CSRF checks, login throttling and IP/CIDR filters.",
    ],
    [
      "Database boundaries",
      "Separate database credentials and permissions. Remote database traffic requires TLS and IP/CIDR rules. MySQL IPv6 networks require individual addresses.",
    ],
    [
      "Administrative audit",
      "Provisioning operations and account changes are recorded. SSH brute-force controls are supplied by Fail2ban.",
    ],
  ];
  $("#content").innerHTML =
    `<div class="info-box mb-5 rounded-xl border px-5 py-4 text-xs leading-6 warning-box border-amber-200/60 bg-amber-50/60 text-amber-900/65">This community alpha has not undergone an independent security audit. Host controls cannot absorb a flood that saturates the server’s network link; arrange upstream DDoS protection with your provider. Use Website protection for WAF and crawler policies, CAPTCHA for verification, Account settings for MFA, and Mail hosting for protected SMTP/IMAP accounts.</div><div class="grid-two grid items-start gap-5 lg:grid-cols-2"><section class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs"><div class="card-head flex items-center justify-between gap-3 border-b border-slate-100 px-5 py-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_small]:text-[9px] [&_small]:font-medium [&_small]:tracking-wider [&_small]:text-slate-400"><h3>Implemented protections</h3><small>CONFIGURATION SUMMARY</small></div>${features.map(([name, detail]) => `<div class="feature-item flex items-start gap-3 border-b border-slate-100 px-5 py-4 last:border-b-0 [&_strong]:text-xs [&_strong]:font-medium [&_p]:mt-1.5 [&_p]:text-[11px] [&_p]:leading-6 [&_p]:text-slate-400"><span class="feature-check mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-emerald-50 text-[10px] text-emerald-600">✓</span><div><strong>${name}</strong><p>${detail}</p></div></div>`).join("")}</section><section><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Security boundaries</h3><p>Tenant commands run inside their own containers. A separate Rust broker performs a fixed set of host operations over a local Unix socket.</p><p>The broker is trusted infrastructure. Keep it updated and restrict host SSH access to administrators.</p><a class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" href="#audit">Review activity →</a></div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Access management</h3><p>Apply source IP rules to tenant panel logins and to each database independently.</p><a class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" href="#${me.role === "admin" ? "users" : "settings"}">Manage accounts →</a></div></section></div>`;
}
async function auditPage() {
  const d = await api("/overview");
  if (current !== "home") return;
  $("#content").innerHTML =
    `<div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs table-wrap overflow-x-auto"><table class="data-table w-full border-collapse whitespace-nowrap text-left [&_th]:bg-slate-50/70 [&_th]:px-5 [&_th]:py-3.5 [&_th]:text-[9px] [&_th]:font-medium [&_th]:uppercase [&_th]:tracking-wider [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-5 [&_td]:py-5 [&_td]:text-xs [&_td_small]:mt-1 [&_td_small]:block [&_td_small]:max-w-48 [&_td_small]:truncate [&_td_small]:text-[10px] [&_td_small]:text-slate-400 [&_tbody_tr]:transition [&_tbody_tr:hover]:bg-slate-50/50"><thead><tr><th>Action</th><th>Actor</th><th>Target</th><th>Time (UTC)</th></tr></thead><tbody>${d.events.map((e) => `<tr><td>${esc(e.action)}</td><td>${esc(e.actor)}</td><td>${esc(e.target)}</td><td>${esc(e.created)}</td></tr>`).join("")}</tbody></table></div><p>Showing the most recent 20 events available to your account.</p>`;
}
async function settings() {
  $("#content").innerHTML =
    `<div class="grid-two grid items-start gap-5 lg:grid-cols-2"><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Your account</h3><p><b>${esc(me.username)}</b> · ${esc(me.role)}</p><button class="primary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition motion-reduce:transition-none hover:bg-emerald-900 disabled:cursor-wait disabled:opacity-50" id="change-password">Change password</button><div id="mfa-settings" class="mt-6 border-t border-slate-100 pt-5"></div></div><div class="card mb-5 overflow-hidden rounded-xl border border-slate-200/70 bg-white shadow-xs aside-card p-5 [&_h3]:mb-4 [&_h3]:text-[12px] [&_h3]:font-semibold [&_p]:mb-4 [&_p]:text-xs [&_p]:text-slate-400"><h3>Current session</h3><p>Sessions expire after eight hours. Password changes revoke all your sessions and API tokens.</p><button class="secondary inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2.5 text-xs font-medium text-slate-600 transition motion-reduce:transition-none hover:border-emerald-200 hover:bg-emerald-50/50 disabled:opacity-50" id="logout">Sign out</button></div></div>`;
  $("#logout").onclick = async () => {
    await api("/logout", "POST", {});
    showLogin();
  };
  const mfaState=await api('/v5/mfa');
  if(!$('#mfa-settings'))return;
  $('#mfa-settings').innerHTML=`<h3>Two-factor authentication</h3><p>${mfaState.enabled ? `Enabled · ${mfaState.recovery_codes_remaining} recovery codes left` : 'Add an authenticator app to protect your account.'}</p><button id="manage-mfa" class="rounded-xl bg-forest px-4 py-2.5 text-white">${mfaState.enabled?'Disable MFA':'Set up MFA'}</button>`;
  $('#manage-mfa').onclick=()=>modal(mfaState.enabled?'Disable MFA':'Set up authenticator',input('password','Current password','password')+(mfaState.enabled?input('code','Authenticator or recovery code'):''),'Continue',async v=>{
    if(mfaState.enabled){await api('/v5/mfa/disable','POST',v);toast('MFA disabled');return render();}
    const enrollment=await api('/v5/mfa/enroll','POST',v);
    setTimeout(()=>modal('Connect your authenticator',`<p class="mb-3 text-sm text-slate-600">Add a time-based account in your authenticator using this secret. Enter its six-digit code below. Setup expires in ten minutes.</p><code class="mb-5 block break-all rounded-xl bg-mint/30 p-4 select-all">${esc(enrollment.secret)}</code>`+input('code','Six-digit code'),'Enable MFA',async value=>{
      const result=await api('/v5/mfa/confirm','POST',value);
      setTimeout(()=>modal('Save your recovery codes',`<p class="mb-3 text-sm">Keep these somewhere safe. Each works once in place of an authenticator code. They will not be shown again.</p><pre class="select-all rounded-xl bg-mint/30 p-4 text-xs">${result.recovery_codes.map(esc).join('\n')}</pre>`,'I saved these codes',async()=>render()),0);
    }),0);
  });
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
