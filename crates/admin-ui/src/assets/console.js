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
  const h = { "Content-Type": "application/json", Accept: "application/json" };
  const t = bearerToken();
  if (t) h.Authorization = "Bearer " + t;
  return h;
}

document.getElementById("create").onclick = async () => {
  const body = {
    namespace: document.getElementById("ns").value,
    name: document.getElementById("name").value,
    type: document.getElementById("ty").value,
  };
  const r = await fetch("/v1/console/containers", {
    method: "POST",
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
