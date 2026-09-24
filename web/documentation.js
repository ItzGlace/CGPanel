export function documentation({
  api,
  esc,
  modal,
  input,
  select,
  textarea,
  toast,
  getMe,
  getCurrent,
}) {
  const $ = (q) => document.querySelector(q);
  const buttonClass =
    "inline-flex items-center justify-center rounded-lg border border-slate-200 bg-white px-4 py-2.5 text-xs font-medium text-slate-600 transition hover:border-emerald-300 hover:bg-emerald-50 motion-reduce:transition-none";
  const primary =
    "inline-flex items-center justify-center rounded-lg bg-forest px-4 py-2.5 text-xs font-medium text-white transition hover:bg-emerald-900 motion-reduce:transition-none";
  const card =
    "rounded-xl border border-slate-200/70 bg-white p-5 shadow-xs motion-safe:animate-enter";
  const code = (s) =>
    `<pre class="my-3 max-w-full overflow-x-auto rounded-lg bg-slate-900 p-4 text-[11px] leading-6 text-emerald-100"><code>${esc(s)}</code></pre>`;
  const date = (s) => (s ? new Date(s * 1000).toLocaleString() : "Never");
  let articles = [],
    selected = "getting-started",
    spec,
    entries = [],
    activeTab = "reference";
  async function read(slug) {
    selected = slug;
    const doc = await api("/docs/" + encodeURIComponent(slug));
    if (getCurrent() !== "docs") return;
    $("#doc-article").innerHTML = doc.html;
    $("#doc-article")
      .querySelectorAll("table")
      .forEach((table) => {
        const wrap = document.createElement("div");
        wrap.className = "my-5 overflow-x-auto";
        table.before(wrap);
        wrap.append(table);
      });
    $("#doc-article")
      .querySelectorAll("a[href^='https://']")
      .forEach((a) => {
        a.target = "_blank";
        a.rel = "noopener noreferrer";
      });
    const headings = [...$("#doc-article").querySelectorAll("h2")];
    headings.forEach((h, i) => {
      h.id = "doc-section-" + i;
    });
    $("#doc-toc").innerHTML = headings
      .map(
        (h, i) =>
          `<button class="block w-full py-2 text-left text-[11px] leading-5 text-slate-500 hover:text-emerald-700" data-doc-heading="doc-section-${i}">${esc(h.textContent)}</button>`,
      )
      .join("");
    document
      .querySelectorAll("[data-doc]")
      .forEach((b) =>
        b.setAttribute(
          "aria-current",
          b.dataset.doc === slug ? "page" : "false",
        ),
      );
  }
  async function guides() {
    articles = await api("/docs");
    if (!articles.some((a) => a.slug === selected)) selected = articles[0].slug;
    $("#content").innerHTML =
      `<div class="grid items-start gap-6 xl:grid-cols-[220px_minmax(0,1fr)]"><aside class="${card} xl:sticky xl:top-6 xl:max-h-[calc(100vh-3rem)] xl:overflow-y-auto"><p class="mb-3 text-[10px] font-semibold uppercase tracking-widest text-slate-400">Guides</p><nav aria-label="Documentation guides" class="space-y-1">${articles.map((a) => `<button data-doc="${a.slug}" class="block w-full rounded-lg px-3 py-2.5 text-left text-xs leading-5 text-slate-600 transition hover:bg-emerald-50 aria-[current=page]:bg-emerald-50 aria-[current=page]:text-emerald-800">${esc(a.title)}</button>`).join("")}</nav><div class="mt-5 hidden border-t border-slate-100 pt-5 xl:block"><p class="text-[10px] font-semibold uppercase tracking-widest text-slate-400">On this page</p><nav id="doc-toc" aria-label="Document sections" class="mt-2"></nav></div><a class="mt-5 block text-xs text-emerald-700 underline" href="https://github.com/ItzGlace/CGPanel/tree/main/docs" target="_blank" rel="noopener">Read on GitHub ↗</a></aside><article id="doc-article" class="${card} min-w-0 p-6 text-sm leading-7 text-slate-600 sm:p-8 [&_h1]:mb-6 [&_h1]:text-2xl [&_h1]:font-semibold [&_h1]:tracking-tight [&_h1]:text-slate-900 [&_h2]:mb-3 [&_h2]:mt-9 [&_h2]:scroll-mt-8 [&_h2]:text-lg [&_h2]:font-semibold [&_h2]:text-slate-800 [&_h3]:mb-3 [&_h3]:mt-6 [&_h3]:font-semibold [&_p]:mb-4 [&_ul]:mb-5 [&_ul]:list-disc [&_ul]:pl-6 [&_ol]:mb-5 [&_ol]:list-decimal [&_ol]:pl-6 [&_li]:mb-2 [&_a]:text-emerald-700 [&_a]:underline [&_pre]:my-5 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-slate-900 [&_pre]:p-5 [&_pre]:text-[11px] [&_pre]:leading-6 [&_pre]:text-emerald-100 [&_code]:font-mono [&_code]:text-[11px] [&_table]:w-full [&_table]:text-left [&_table]:text-xs [&_th]:border-b [&_th]:border-slate-200 [&_th]:p-3 [&_th]:text-slate-800 [&_td]:border-b [&_td]:border-slate-100 [&_td]:p-3 [&_td]:align-top"></article></div>`;
    await read(selected);
  }
  function curlFor(entry) {
    const { path, method, operation: op } = entry;
    let result = `curl --fail-with-body --silent --show-error --request ${method.toUpperCase()}`;
    const sessionOnly =
      op.security?.length && !op.security.some((s) => s.AdminBearer);
    if (op.security?.length)
      result += sessionOnly
        ? " \\\n  --cookie cookies.txt"
        : " \\\n  --header @auth-header.txt";
    if (sessionOnly && method !== "get")
      result += " \\\n  --header @csrf-header.txt";
    const body = op.requestBody?.content?.["application/json"];
    if (body)
      result +=
        ' \\\n  --header "Content-Type: application/json" \\\n  --data-binary @request.json';
    result += ` \\\n  "${location.origin}${path}"`;
    return result;
  }
  function endpoints() {
    const filter = ($("#api-search")?.value || "").toLowerCase();
    const category = $("#api-category")?.value || "";
    const shown = entries.filter(
      (e) =>
        (!category || e.operation.tags.includes(category)) &&
        `${e.method} ${e.path} ${e.operation.summary} ${e.operation.description}`
          .toLowerCase()
          .includes(filter),
    );
    $("#endpoint-count").textContent =
      `${shown.length} of ${entries.length} operations`;
    $("#endpoint-list").innerHTML =
      shown
        .map((entry) => {
          const { path, method, operation: op, index } = entry;
          const body = op.requestBody?.content?.["application/json"];
          const schema = body?.schema?.$ref
            ? spec.components.schemas[body.schema.$ref.split("/").pop()]
            : body?.schema;
          const params = op.parameters?.length
            ? `<h4 class="mt-5 text-xs font-semibold">Parameters</h4>${code(JSON.stringify(op.parameters, null, 2))}`
            : "";
          const badge =
            method === "get"
              ? "bg-emerald-50 text-emerald-700"
              : method === "delete"
                ? "bg-red-50 text-red-600"
                : "bg-amber-50 text-amber-700";
          return `<details class="${card} mb-3 p-0"><summary class="flex cursor-pointer flex-wrap items-center gap-3 p-4 text-xs sm:p-5"><span class="rounded px-2.5 py-1.5 font-mono text-[10px] font-semibold ${badge}">${method.toUpperCase()}</span><code class="min-w-0 break-all font-medium text-slate-800">${esc(path)}</code><span class="text-slate-400 sm:ml-auto">${esc(op.summary)}</span></summary><div class="border-t border-slate-100 p-5"><p class="text-xs leading-6 text-slate-600">${esc(op.description)}</p><p class="mt-2 text-[11px] text-slate-400">${!op.security.length ? "Public endpoint" : op.security.some((s) => s.AdminBearer) ? "Bearer token or permitted session; role and ownership checks still apply." : "Session authentication only; CSRF required for mutations."}</p>${params}${body?.example ? `<h4 class="mt-5 text-xs font-semibold">request.json example</h4>${code(JSON.stringify(body.example, null, 2))}` : ""}${schema ? `<details class="my-4"><summary class="cursor-pointer text-xs font-medium text-emerald-700">Request schema</summary>${code(JSON.stringify(schema, null, 2))}<p class="text-[11px] text-slate-500">Referenced schemas are included in the OpenAPI download and the full endpoint guide.</p></details>` : ""}<div class="mt-5 flex items-center justify-between gap-3"><h4 class="text-xs font-semibold">curl example</h4><button class="${buttonClass}" data-api-copy="${index}">Copy example</button></div>${code(curlFor(entry))}<p class="text-[11px] leading-6 text-slate-500">Replace path placeholders and set the panel origin. Add query parameters where needed. Save the example body in request.json and the authorization header in a private auth-header.txt. The API guide explains session cookies and CSRF files.</p><details class="mt-4"><summary class="cursor-pointer text-xs font-medium text-emerald-700">Responses</summary>${code(JSON.stringify(op.responses, null, 2))}</details></div></details>`;
        })
        .join("") ||
      `<div class="${card} text-center text-sm text-slate-500">No matching endpoints.</div>`;
  }
  async function tokensView() {
    const items = await api("/admin/tokens");
    if (getCurrent() !== "admin-api" || activeTab !== "tokens") return;
    $("#api-body").innerHTML =
      `<div class="${card}"><div class="flex flex-wrap items-center justify-between gap-4"><div><h2 class="text-sm font-semibold">Automation tokens</h2><p class="mt-2 text-xs leading-6 text-slate-500">Secrets are shown once. Choose an expiry and optional source IP rules.</p></div><button class="${primary}" data-token-create>Create token</button></div><p class="mt-5 rounded-lg bg-amber-50 p-4 text-xs leading-6 text-amber-900">Full-admin tokens can provision and delete resources. Read-only tokens still access sensitive readable information and backup downloads. Tokens cannot manage credentials or change your password.</p><div class="mt-5 overflow-x-auto"><table class="w-full text-left text-xs [&_th]:px-3 [&_th]:py-3 [&_th]:font-medium [&_th]:text-slate-400 [&_td]:border-t [&_td]:border-slate-100 [&_td]:px-3 [&_td]:py-4"><thead><tr><th>Name / prefix</th><th>Access</th><th>Expires</th><th>Last used</th><th>Status</th><th></th></tr></thead><tbody>${
        items
          .map((t) => {
            const expired = t.expires * 1000 <= Date.now(),
              status = t.revoked ? "Revoked" : expired ? "Expired" : "Active";
            return `<tr><td>${esc(t.name)}<code class="mt-1 block text-[10px] text-slate-400">${esc(t.prefix)}…</code><span class="mt-1 block text-[10px] text-slate-400">${esc(t.allowed_ips.join(", ") || "Account IP rules only")}</span></td><td>${t.scope === "read" ? "Read only" : "Full admin"}</td><td class="whitespace-nowrap">${date(t.expires)}</td><td class="whitespace-nowrap">${date(t.last_used)}</td><td>${status}</td><td>${!t.revoked && !expired ? `<button class="${buttonClass}" data-token-revoke="${t.id}" data-token-name="${esc(t.name)}">Revoke</button>` : ""}</td></tr>`;
          })
          .join("") ||
        '<tr><td colspan="6" class="py-8 text-center text-slate-400">No API tokens yet.</td></tr>'
      }</tbody></table></div></div>`;
  }
  async function apiPage() {
    if (getMe().role !== "admin")
      throw new Error("Administrator access required.");
    spec = await api("/admin/openapi.json");
    entries = Object.entries(spec.paths)
      .flatMap(([path, methods]) =>
        Object.entries(methods)
          .filter(([method]) =>
            ["get", "post", "delete", "put", "patch"].includes(method),
          )
          .map(([method, operation]) => ({ path, method, operation })),
      )
      .map((e, index) => ({ ...e, index }));
    $("#content").innerHTML =
      `<div class="mb-6 flex flex-wrap items-center justify-between gap-4"><p class="max-w-2xl text-xs leading-6 text-slate-500">Automate your hosting workspace with documented requests, scoped access and revocable credentials. ${entries.length} operations · OpenAPI ${esc(spec.openapi)} · CGPanel ${esc(spec.info.version)}</p><div class="flex flex-wrap gap-2"><button class="${buttonClass}" data-api-guide>Read API guide</button><a class="${buttonClass}" href="/api/admin/openapi.json" download>Download OpenAPI</a></div></div><div class="mb-6 flex gap-2" role="tablist" aria-label="API center"><button role="tab" data-api-tab="reference" class="${buttonClass} aria-selected:bg-emerald-50 aria-selected:text-emerald-800">Endpoints</button><button role="tab" data-api-tab="tokens" class="${buttonClass} aria-selected:bg-emerald-50 aria-selected:text-emerald-800">API tokens</button></div><div id="api-body" role="tabpanel"></div>`;
    await tab(activeTab);
  }
  async function tab(name) {
    activeTab = name;
    document
      .querySelectorAll("[data-api-tab]")
      .forEach((b) =>
        b.setAttribute("aria-selected", String(b.dataset.apiTab === name)),
      );
    if (name === "tokens") return tokensView();
    $("#api-body").innerHTML =
      `<div class="mb-5 flex flex-wrap items-end gap-3"><label class="min-w-0 flex-1 text-xs text-slate-500">Search endpoints<input id="api-search" type="search" placeholder="Try backups, users, token…" class="mt-2 block w-full rounded-lg border border-slate-200 bg-white px-4 py-3 text-sm outline-none focus:border-emerald-500"></label><label class="text-xs text-slate-500">Category<select id="api-category" class="mt-2 block max-w-full rounded-lg border border-slate-200 bg-white px-4 py-3 text-sm"><option value="">All categories</option>${[...new Set(entries.flatMap((e) => e.operation.tags))].map((tag) => `<option>${esc(tag)}</option>`).join("")}</select></label></div><p id="endpoint-count" class="mb-4 text-[11px] text-slate-400" aria-live="polite"></p><div id="endpoint-list"></div>`;
    $("#api-search").addEventListener("input", endpoints);
    $("#api-category").addEventListener("change", endpoints);
    endpoints();
  }
  function createToken() {
    modal(
      "Create administrator API token",
      input("name", "Token name") +
        select("scope", "Access", [
          ["read", "Read only"],
          ["admin", "Full administrator"],
        ]) +
        input(
          "expires_days",
          "Expires in days",
          "number",
          "1–90 days; default 30.",
          "30",
        ) +
        textarea(
          "allowed_ips",
          "Allowed source CIDRs",
          "Optional, one CIDR per line. Both token and account rules apply.",
        ).replaceAll(" required", ""),
      "Create token",
      async (v) => {
        const ips = v.allowed_ips.split(/[\s,]+/).filter(Boolean);
        const result = await api("/admin/tokens", "POST", {
          name: v.name,
          scope: v.scope,
          expires_days: Number(v.expires_days),
          allowed_ips: ips,
        });
        $("#dialog").close();
        await tokensView().catch(() => {});
        modal(
          "Save your API token",
          `<p class="mb-4 text-xs leading-6 text-slate-500">This secret is shown once. Store it privately before closing. It expires ${esc(date(result.expires))}.</p><label class="block text-xs font-medium">API token<textarea id="new-api-token" readonly autocomplete="off" spellcheck="false" class="mt-3 block w-full break-all rounded-lg border border-slate-200 bg-slate-50 p-4 font-mono text-xs" rows="4">${esc(result.token)}</textarea></label>`,
          "",
          null,
        );
        $("#dialog").addEventListener(
          "close",
          () => {
            const value = $("#new-api-token");
            if (value) {
              value.value = "";
              value.textContent = "";
            }
          },
          { once: true },
        );
      },
    );
  }
  document.addEventListener("click", async (event) => {
    const b = event.target.closest(
      "[data-doc],[data-doc-heading],[data-api-tab],[data-api-guide],[data-api-copy],[data-token-create],[data-token-revoke]",
    );
    if (!b) return;
    try {
      if (b.dataset.doc) await read(b.dataset.doc);
      else if (b.dataset.docHeading)
        document
          .getElementById(b.dataset.docHeading)
          ?.scrollIntoView({
            behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
              ? "instant"
              : "smooth",
            block: "start",
          });
      else if (b.dataset.apiTab) await tab(b.dataset.apiTab);
      else if (b.hasAttribute("data-api-guide")) {
        selected = "api";
        location.hash = "docs";
      } else if (b.hasAttribute("data-api-copy")) {
        await navigator.clipboard.writeText(
          curlFor(entries[Number(b.dataset.apiCopy)]),
        );
        toast("Example copied. Replace placeholders before running.");
      } else if (b.hasAttribute("data-token-create")) createToken();
      else if (b.dataset.tokenRevoke)
        modal(
          "Revoke API token?",
          `<p class="text-sm leading-7">Future requests from <strong>${esc(b.dataset.tokenName)}</strong> will be rejected. Already running requests and queued jobs are not cancelled.</p>`,
          "Revoke token",
          async () => {
            await api("/admin/tokens/" + b.dataset.tokenRevoke, "DELETE");
            $("#dialog").close();
            await tokensView();
            toast("Token revoked.");
          },
        );
    } catch (e) {
      toast(e.message);
    }
  });
  return { render: (page) => (page === "docs" ? guides() : apiPage()) };
}
