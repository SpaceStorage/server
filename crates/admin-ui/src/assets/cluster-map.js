(async function () {
  const interval = (window.SS_MAP_INTERVAL || 2) * 1000;
  const el = document.getElementById("map");
  const statusEl = document.getElementById("map-status");

  function authHeaders() {
    return { Accept: "application/json" };
  }

  function esc(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function shortId(id) {
    const s = String(id || "");
    return s.length > 12 ? s.slice(0, 8) + "…" : s;
  }

  function render(j) {
    const nodes = Array.isArray(j.nodes) ? j.nodes : [];
    const containers = Array.isArray(j.containers) ? j.containers : [];
    const ready = nodes.filter((n) => n.ready).length;
    const scope = j.scope || "cluster";
    // Poll rebuilds #map; keep <details> open across refreshes.
    const rawOpen = !!(el.querySelector("details.raw-json") || {}).open;

    const mapStale =
      shellHosted() &&
      typeof window.SS_SHELL_MAP_STALE === "function" &&
      window.SS_SHELL_MAP_STALE();
    if (statusEl) {
      statusEl.className = mapStale ? "banner need-login" : "banner ok";
      statusEl.innerHTML =
        "<h2>" +
        (mapStale ? "Cluster map stale" : "Cluster live") +
        "</h2><p>" +
        esc(ready) +
        " of " +
        esc(nodes.length) +
        " nodes ready · scope <strong>" +
        esc(scope) +
        "</strong></p>";
    }

    let html = "";
    html += '<div class="panel">';
    html += '<div class="stats">';
    html +=
      '<div class="stat"><strong>' +
      esc(nodes.length) +
      '</strong><span>Nodes</span></div>';
    html +=
      '<div class="stat"><strong>' +
      esc(ready) +
      '</strong><span>Ready</span></div>';
    html +=
      '<div class="stat"><strong>' +
      esc(containers.length) +
      '</strong><span>Containers</span></div>';
    html += "</div>";

    html += "<h2>Nodes</h2>";
    if (!nodes.length) {
      html += '<p class="muted">No members in the map yet.</p>';
    } else {
      html += "<table><thead><tr><th>Name</th><th>Status</th><th>Node id</th></tr></thead><tbody>";
      for (const n of nodes) {
        const pill = n.ready
          ? '<span class="pill ready">ready</span>'
          : '<span class="pill down">not ready</span>';
        html +=
          "<tr><td><strong>" +
          esc(n.name || "") +
          "</strong></td><td>" +
          pill +
          '</td><td class="mono muted" title="' +
          esc(n.id || "") +
          '">' +
          esc(shortId(n.id)) +
          "</td></tr>";
      }
      html += "</tbody></table>";
    }
    html += "</div>";

    html += '<div class="panel"><h2>Containers</h2>';
    if (!containers.length) {
      html +=
        '<p class="muted">None yet. Use <a href="/ui/console">Console</a> to create one after login.</p>';
    } else {
      html +=
        "<table><thead><tr><th>Namespace</th><th>Name</th><th>Type</th><th>Replicas</th></tr></thead><tbody>";
      for (const c of containers) {
        const shards = Array.isArray(c.shards) ? c.shards : [];
        let reps = 0;
        for (const s of shards) {
          reps += Array.isArray(s.replicas) ? s.replicas.length : 0;
        }
        html +=
          "<tr><td>" +
          esc(c.namespace || "") +
          "</td><td><strong>" +
          esc(c.name || "") +
          "</strong></td><td>" +
          esc(c.type_name || c.type || "") +
          "</td><td>" +
          esc(reps) +
          "</td></tr>";
      }
      html += "</tbody></table>";
    }
    html += "</div>";

    html +=
      '<details class="raw-json"' +
      (rawOpen ? " open" : "") +
      '><summary>Raw JSON</summary><pre class="raw">' +
      esc(JSON.stringify(j, null, 2)) +
      "</pre></details>";

    el.innerHTML = html;
  }

  function shellHosted() {
    const dp = document.body && document.body.getAttribute("data-path");
    return !!(dp && dp.indexOf("/ui/") === 0);
  }

  function mapUrl() {
    const ns = window.SS_SHELL_MAP_NS;
    if (shellHosted() && !ns) {
      return null;
    }
  if (ns) {
      return "/v1/cluster/map?namespace=" + encodeURIComponent(ns);
    }
    return "/v1/cluster/map";
  }

  async function refresh() {
    try {
      const url = mapUrl();
      if (!url) {
        if (statusEl) {
          statusEl.className = "banner muted";
          statusEl.innerHTML = "<h2>Select a namespace</h2><p>Pick a namespace to load the cluster map.</p>";
        }
        if (el) el.innerHTML = "";
        return false;
      }
      const r = await fetch(url, {
        credentials: "same-origin",
        headers: authHeaders(),
      });
      if (r.status === 401 || r.status === 403) {
        if (statusEl) {
          statusEl.className = "banner need-login";
          statusEl.innerHTML =
            "<h2>Session expired or invalid</h2><p>Log in again with the admin token.</p>";
        }
        el.innerHTML = "";
        return false;
      }
      if (!r.ok) {
        if (statusEl) {
          statusEl.className = "banner err";
          statusEl.innerHTML = "<h2>Could not load map</h2><p>HTTP " + r.status + "</p>";
        }
        el.innerHTML = "";
        return false;
      }
      render(await r.json());
      return true;
    } catch (e) {
      if (statusEl) {
        statusEl.className = "banner err";
        statusEl.innerHTML = "<h2>Could not load map</h2><p>" + esc(e) + "</p>";
      }
      el.innerHTML = "";
      return false;
    }
  }

  if (!shellHosted()) {
    refresh();
    setInterval(refresh, interval);
  } else {
    window.SS_SHELL_MAP_RENDER = refresh;
  }
})();
