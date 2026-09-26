(async function () {
  const interval = (window.SS_MAP_INTERVAL || 2) * 1000;
  const el = document.getElementById("map");
  async function refresh() {
    try {
      const r = await fetch("/v1/cluster/map", { credentials: "same-origin" });
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
