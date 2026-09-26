function authHeaders() {
  return { "Content-Type": "application/json" };
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
