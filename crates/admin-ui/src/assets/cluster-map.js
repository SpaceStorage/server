(async function () {
  const interval = (window.SS_MAP_INTERVAL || 2) * 1000;
  const el = document.getElementById("map");

  function bearerToken() {
    const q = new URLSearchParams(window.location.search).get("token");
    if (q) {
      try {
        sessionStorage.setItem("ss_token", q);
      } catch (_) {}
      return q;
    }
    try {
      return sessionStorage.getItem("ss_token") || window.SS_TOKEN || "";
    } catch (_) {
      return window.SS_TOKEN || "";
    }
  }

  function authHeaders() {
    const h = { Accept: "application/json" };
    const t = bearerToken();
    if (t) h.Authorization = "Bearer " + t;
    return h;
  }

  async function refresh() {
    try {
      const r = await fetch("/v1/cluster/map", {
        credentials: "same-origin",
        headers: authHeaders(),
      });
      if (!r.ok) {
        el.textContent = "HTTP " + r.status;
        return;
      }
      const j = await r.json();
      el.textContent = JSON.stringify(j, null, 2);
    } catch (e) {
      el.textContent = String(e);
    }
  }
  refresh();
  setInterval(refresh, interval);
})();
