(function () {
  "use strict";

  function authHeaders() {
    const h = {
      "Content-Type": "application/json",
      Accept: "application/json",
      Origin: location.origin,
    };
    if (window.SS_SHELL && window.SS_SHELL.cookieMutationHeaders) {
      Object.assign(h, window.SS_SHELL.cookieMutationHeaders());
    }
    return h;
  }

  function hasCookieSession() {
    return !!(window.SS_SHELL && window.SS_SHELL.cookieMutationHeaders);
  }

  function showNeedLogin() {
    const b = document.getElementById("console-status");
    if (!b) return;
    if (hasCookieSession()) {
      b.className = "banner ok";
      b.innerHTML =
        "<h2>Logged in</h2><p>Create a container, then browse its rows. Namespace must exist (or use cluster admin).</p>";
    } else {
      b.className = "banner need-login";
      b.innerHTML =
        "<h2>Log in first</h2><p>Use the shell sign-in, then return to Browse.</p>";
    }
  }

  function applyBrowseDeepLink() {
    const qs = new URLSearchParams(location.search);
    const ns = qs.get("namespace");
    if (!ns) return;
    const el = document.getElementById("ns");
    if (el) el.value = ns;
  }

  function wireConsole() {
    const create = document.getElementById("create");
    if (!create || create.getAttribute("data-wired")) return;
    create.setAttribute("data-wired", "1");
    create.onclick = async () => {
      const body = {
        namespace: document.getElementById("ns").value,
        name: document.getElementById("name").value,
        type: document.getElementById("ty").value,
      };
      const r = await fetch("/v1/console/containers", {
        method: "POST",
        credentials: "same-origin",
        headers: authHeaders(),
        body: JSON.stringify(body),
      });
      document.getElementById("out").textContent = await r.text();
    };
    document.getElementById("browse").onclick = async () => {
      const body = {
        namespace: document.getElementById("ns").value,
        container: document.getElementById("name").value,
        limit: 100,
      };
      const r = await fetch("/v1/console/query", {
        method: "POST",
        credentials: "same-origin",
        headers: authHeaders(),
        body: JSON.stringify(body),
      });
      document.getElementById("out").textContent = await r.text();
    };

    async function postConfig(settings) {
      const scope = document.getElementById("cfg-scope").value;
      const body = {
        scope,
        namespace: document.getElementById("ns").value,
        settings: settings || {},
      };
      const r = await fetch("/v1/console/config", {
        method: "POST",
        credentials: "same-origin",
        headers: authHeaders(),
        body: JSON.stringify(body),
      });
      document.getElementById("cfg-out").textContent = await r.text();
    }

    document.getElementById("cfg-load").onclick = async () => {
      await postConfig({});
    };
    document.getElementById("cfg-save").onclick = async () => {
      let settings = {};
      try {
        settings = JSON.parse(document.getElementById("cfg-settings").value || "{}");
      } catch (e) {
        document.getElementById("cfg-out").textContent = "invalid JSON: " + e;
        return;
      }
      await postConfig(settings);
    };
  }

  function initConsole() {
    applyBrowseDeepLink();
    wireConsole();
    showNeedLogin();
  }

  window.SS_CONSOLE_INIT = initConsole;

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initConsole);
  } else {
    initConsole();
  }
})();