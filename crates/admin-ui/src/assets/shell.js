/* SpaceStorage admin shell — sole poller (10s). specs/018 contracts/shell-http.md */
(function () {
  'use strict';
  var CSRF_HEADER = 'X-SpaceStorage-CSRF';
  var POLL_MS = 10000;
  var STALE_MS = 30000;
  var TRUNC_LEN = 48;
  var sessionView = null;
  var csrfToken = '';
  var pollTimer = null;
  var pollPaused = false;
  var overviewInflight = false;
  var lastGeneratedAt = null;
  var lastMapAt = null;
  var staleTickTimer = null;
  var lastNodesRows = [];
  var pagingAppend = false;

  function esc(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function trunc(s, max) {
    var t = String(s || '');
    if (t.length <= max) return { short: esc(t), title: '' };
    return { short: esc(t.slice(0, max)) + '…', title: esc(t) };
  }

  function path() {
    return (document.body && document.body.getAttribute('data-path')) || '/ui/overview';
  }

  function $(id) {
    return document.getElementById(id);
  }

  function showLogin(preserveFields) {
    var lp = $('login-panel');
    var nav = document.querySelector('nav.shell-nav');
    var mount = $('section-mount');
    var refuse = $('section-refusal');
    if (lp) lp.hidden = false;
    if (nav) nav.hidden = true;
    if (mount) mount.hidden = true;
    if (refuse) refuse.hidden = true;
    var next = path();
    if (next.indexOf('/ui/') === 0) {
      var u = new URL(location.href);
      if (!u.searchParams.get('next')) u.searchParams.set('next', next);
      history.replaceState(null, '', u.pathname + u.search);
    }
    if (!preserveFields) {
      var tok = $('admin-tok');
      if (tok) tok.value = '';
    }
  }

  function formatExpiry(expiresIn) {
    if (!expiresIn) return '';
    var m = Math.floor(expiresIn / 60);
    return 'session expires in ' + m + ' min';
  }

  function filterNav(aff, scope) {
    if (!aff) return;
    document.querySelectorAll('nav.shell-nav a[data-aff]').forEach(function (a) {
      var key = a.getAttribute('data-aff');
      if (key && aff[key] === false) a.hidden = true;
    });
    if (scope === 'namespace') {
      document.querySelectorAll('nav.shell-nav a[href="/ui/audit"]').forEach(function (a) {
        a.hidden = true;
      });
      var ns = (sessionView && sessionView.namespaces && sessionView.namespaces[0]) || '';
      document.querySelectorAll('nav.shell-nav a[href="/ui/monitoring"]').forEach(function (a) {
        if (ns) a.setAttribute('href', '/ui/monitoring/namespaces/' + encodeURIComponent(ns));
      });
    }
  }

  function withPageQuery(base) {
    var qs = new URLSearchParams(location.search);
    var q = [];
    ['cursor', 'limit', 'q', 'status'].forEach(function (k) {
      var v = qs.get(k);
      if (v) q.push(k + '=' + encodeURIComponent(v));
    });
    return q.length ? base + '?' + q.join('&') : base;
  }

  function sectionAgeRoots() {
    var roots = [];
    var p = path();
    if (p.indexOf('/ui/overview') === 0) {
      var o = $('overview-panel');
      if (o) roots.push(o);
    } else if (p.indexOf('/ui/map') === 0) {
      var m = $('map-status');
      if (m) roots.push(m);
    } else {
      var s = $('section-panel');
      if (s) roots.push(s);
      else {
        var m = $('section-mount');
        if (m) roots.push(m);
      }
      var nm = $('namespaces-panel');
      if (nm) roots.push(nm);
      var nd = $('nodes-panel');
      if (nd) roots.push(nd);
    }
    return roots;
  }

  function applyAgeToRoot(root, iso, liveLabel) {
    if (!root || !iso) return;
    var age = relAge(iso);
    var p = root.querySelector('.panel-age');
    if (!p) {
      p = document.createElement('p');
      p.className = 'panel-age';
      root.insertBefore(p, root.firstChild);
    }
    var live = liveLabel ? ' · ' + liveLabel : '';
    p.textContent = age.rel + ' · ' + age.abs + live;
    if (age.stale) p.classList.add('status-stale');
    else p.classList.remove('status-stale');
  }

  function showSectionReadError(status, body) {
    var msg =
      (body && (body.message || body.code)) ||
      ('read failed (HTTP ' + status + ')');
    var el = $('section-panel') || $('overview-panel') || $('namespaces-panel') || $('nodes-panel') || $('section-mount');
    if (!el) return;
    var mount = $('section-mount');
    if (mount) mount.hidden = false;
    var refuse = $('section-refusal');
    if (refuse) refuse.hidden = true;
    el.innerHTML =
      '<div class="callout err" role="alert"><strong>Could not load panel</strong><p>' +
      esc(msg) +
      '</p></div>';
  }

  function bindLoadMore(nextCursor) {
    if (!nextCursor) return '';
    return (
      '<p class="load-more-row"><button type="button" class="secondary" data-next-cursor="' +
      esc(nextCursor) +
      '">Load more</button></p>'
    );
  }

  function wireLoadMore(root) {
    if (!root) return;
    var btn = root.querySelector('button[data-next-cursor]');
    if (!btn) return;
    btn.addEventListener('click', function () {
      var cur = btn.getAttribute('data-next-cursor');
      if (!cur) return;
      pagingAppend = true;
      var u = new URL(location.href);
      u.searchParams.set('cursor', cur);
      history.replaceState(null, '', u.pathname + u.search);
      loadSection();
    });
  }

  function attachSession(view) {
    sessionView = view;
    csrfToken = (view && view.csrf_token) || '';
    var lp = $('login-panel');
    var nav = document.querySelector('nav.shell-nav');
    var mount = $('section-mount');
    if (lp) lp.hidden = true;
    if (nav) nav.hidden = false;
    if (mount) mount.hidden = false;
    var who = $('principal-label');
    if (who && view) who.textContent = view.principal || '';
    var scope = $('scope-label');
    if (scope && view) {
      var ns = (view.namespaces || []).join(', ');
      scope.textContent = view.scope === 'cluster' ? 'scope: cluster' : 'scope: ' + (ns || 'namespace');
    }
    var exp = $('session-expiry');
    if (exp && view) exp.textContent = formatExpiry(view.expires_in);
    filterNav(view && view.affordances, view && view.scope);
    var reloadBtn = $('config-reload');
    if (reloadBtn) {
      var show = view && view.affordances && view.affordances.config_reload;
      reloadBtn.hidden = !show;
    }
    window.SS_SHELL_MAP_STALE = function () {
      return !!(lastMapAt && relAge(lastMapAt).stale);
    };
    loadSection();
    schedulePoll();
    if (path().indexOf('/ui/browse') === 0 && window.SS_CONSOLE_INIT) window.SS_CONSOLE_INIT();
  }

  function cookieMutationHeaders() {
    return {
      'Content-Type': 'application/json',
      Origin: location.origin,
      [CSRF_HEADER]: csrfToken,
    };
  }

  function apiPathForSection() {
    var p = path();
    if (p === '/ui/overview' || p.indexOf('/ui/overview') === 0) return '/v1/ui/overview';
    if (p === '/ui/namespaces') {
      return withPageQuery('/v1/ui/namespaces');
    }
    var nsParts = p.split('/');
    if (p.indexOf('/ui/namespaces/') === 0 && nsParts.length === 4 && nsParts[3]) {
      return withPageQuery('/v1/ui/namespaces/' + encodeURIComponent(nsParts[3]));
    }
    if (p === '/ui/nodes') {
      return withPageQuery('/v1/ui/nodes');
    }
    var pathParts = p.split('/');
    if (p.indexOf('/ui/nodes/') === 0 && pathParts.length === 4 && pathParts[3]) {
      return '/v1/ui/nodes/' + encodeURIComponent(pathParts[3]);
    }
    if (p === '/ui/monitoring') return '/v1/ui/monitoring';
    if (p.indexOf('/ui/monitoring/namespaces/') === 0 && pathParts.length === 5 && pathParts[4]) {
      return '/v1/ui/monitoring/namespaces/' + encodeURIComponent(pathParts[4]);
    }
    if (p === '/ui/jobs') return withPageQuery('/v1/ui/jobs');
    if (p.indexOf('/ui/jobs/') === 0 && pathParts.length === 4 && pathParts[3]) {
      return '/v1/ui/jobs/' + encodeURIComponent(pathParts[3]);
    }
    if (p === '/ui/audit') return withPageQuery('/v1/ui/audit');
    return null;
  }

  function confirmNormal(message) {
    return window.confirm(message);
  }

  function confirmStrong(message, expected) {
    var typed = window.prompt(message, '');
    if (typed === null) return false;
    return typed.trim() === expected;
  }

  function showGuardOutcome(text) {
    var el = $('guard-outcome');
    if (el) el.textContent = text || '';
  }

  async function guardedMutation(label, run) {
    try {
      var out = await run();
      if (!out.ok) {
        var msg = (out.body && out.body.message) || label + ' refused';
        showGuardOutcome(msg);
        return { ok: false, unconfirmed: false, body: out.body };
      }
      showGuardOutcome(label + ' accepted');
      return { ok: true, body: out.body };
    } catch (e) {
      showGuardOutcome('unconfirmed — re-read server state');
      if (typeof loadSection === 'function') loadSection();
      return { ok: false, unconfirmed: true };
    }
  }

  async function postJson(url, body) {
    var r = await fetch(url, {
      method: 'POST',
      credentials: 'same-origin',
      headers: cookieMutationHeaders(),
      body: body ? JSON.stringify(body) : '{}',
    });
    var j = await r.json().catch(function () { return {}; });
    return { ok: r.ok, status: r.status, body: j };
  }

  async function adminPost(opBody) {
    var r = await fetch('/v1/admin', {
      method: 'POST',
      credentials: 'same-origin',
      headers: cookieMutationHeaders(),
      body: JSON.stringify(opBody),
    });
    var j = await r.json().catch(function () { return {}; });
    return { ok: r.ok, status: r.status, body: j };
  }

  function countLabel(n) {
    if (!n) return '0';
    return n.container_count_complete ? String(n.container_count) : 'at least ' + n.container_count;
  }

  function confirmCascadeDelete(name, count, complete) {
    var cnt = complete ? String(count) : 'at least ' + count;
    return window.confirm(
      'Delete namespace ' + name + '? This will remove ' + cnt + ' container(s) and cannot be undone.'
    );
  }

  function bindNamespaceActions(root) {
    if (!root || root.getAttribute('data-ns-bound')) return;
    root.setAttribute('data-ns-bound', '1');
    var createBtn = root.querySelector('#ns-create');
    if (createBtn) {
      createBtn.addEventListener('click', function () {
        root.innerHTML =
          '<form id="ns-create-form" class="stacked">' +
          '<label>Name <input type="text" id="ns-name" autocomplete="off"></label>' +
          '<p id="ns-field-error" class="field-error" role="alert"></p>' +
          '<button type="submit" class="primary">Create</button></form>';
        var form = $('ns-create-form');
        if (form) {
          form.addEventListener('submit', async function (ev) {
            ev.preventDefault();
            var nm = ($('ns-name') && $('ns-name').value.trim()) || '';
            var fe = $('ns-field-error');
            if (!nm) {
              if (fe) fe.textContent = 'name required';
              return;
            }
            var out = await adminPost({ op: 'namespace_create', name: nm });
            if (!out.ok) {
              if (fe) fe.textContent = (out.body && out.body.message) || 'create refused';
              return;
            }
            if (fe) fe.textContent = '';
            loadSection();
          });
        }
      });
    }
    var list = root.querySelector('.ns-list');
    if (list && sessionView && sessionView.affordances && sessionView.affordances.namespace_write) {
      var bar = document.createElement('p');
      bar.innerHTML = '<button type="button" id="ns-create" class="primary">Create namespace</button>';
      root.insertBefore(bar, list);
      bindNamespaceActions(root);
    }
  }

  function bindNamespaceDetail(root, data) {
    if (!root || root.getAttribute('data-ns-detail-bound')) return;
    if (!sessionView || !sessionView.affordances || !sessionView.affordances.namespace_write) return;
    root.setAttribute('data-ns-detail-bound', '1');
    var tools = document.createElement('div');
    tools.className = 'ns-tools';
    tools.innerHTML =
      '<button type="button" id="ns-rename-btn">Rename</button> ' +
      '<button type="button" id="ns-delete-btn">Delete</button>';
    if (sessionView.affordances.quota_replace) {
      tools.innerHTML +=
        ' <button type="button" id="ns-quota-btn">Replace quota</button>';
    }
    root.appendChild(tools);
    var fieldErr = document.createElement('p');
    fieldErr.id = 'ns-field-error';
    fieldErr.className = 'field-error';
    fieldErr.setAttribute('role', 'alert');
    root.appendChild(fieldErr);
    $('ns-delete-btn').addEventListener('click', async function () {
      var cnt = data.container_count || 0;
      var ok = data.container_count ? confirmCascadeDelete(data.name, cnt, data.container_count_complete) : window.confirm('Delete namespace ' + data.name + '?');
      if (!ok) return;
      var out = await adminPost({ op: 'namespace_delete', name: data.name, cascade: true });
      if (!out.ok && out.body && out.body.code === 'CascadeRequired') {
        if (confirmCascadeDelete(data.name, cnt, data.container_count_complete)) {
          out = await adminPost({ op: 'namespace_delete', name: data.name, cascade: true });
        } else {
          return;
        }
      }
      if (!out.ok) {
        fieldErr.textContent = (out.body && out.body.message) || 'delete refused';
        return;
      }
      location.href = '/ui/namespaces';
    });
    $('ns-rename-btn').addEventListener('click', async function () {
      var nn = window.prompt('New namespace name');
      if (!nn) return;
      var out = await adminPost({ op: 'namespace_rename', name: data.name, new_name: nn });
      if (!out.ok) {
        fieldErr.textContent = (out.body && out.body.message) || 'rename refused';
        return;
      }
      location.href = '/ui/namespaces/' + encodeURIComponent(nn);
    });
    var qb = $('ns-quota-btn');
    if (qb) {
      qb.addEventListener('click', async function () {
        var bytes = window.prompt('Quota bytes (integer) for namespace ' + data.name);
        if (bytes === null) return;
        var lim = parseInt(bytes, 10);
        if (!window.confirm('Replace quota for namespace ' + data.name + ' with ' + lim + ' bytes?')) return;
        var out = await adminPost({
          op: 'quota_replace',
          name: data.name,
          quotas: [{ unit: 'bytes', limit: lim || 0 }],
        });
        if (!out.ok) {
          fieldErr.textContent = (out.body && out.body.message) || 'quota refused';
          return;
        }
        loadSection();
      });
    }
  }

  function showRefusal(message) {
    var refuse = $('section-refusal');
    var mount = $('section-mount');
    if (mount) mount.hidden = true;
    if (refuse) {
      refuse.hidden = false;
      refuse.textContent = message || 'Forbidden';
    }
  }

  function ensureMapRenderHook() {
    if (!window.SS_SHELL_MAP_RENDER || window.SS_SHELL_MAP_RENDER.__ssHooked) return;
    var prev = window.SS_SHELL_MAP_RENDER;
    window.SS_SHELL_MAP_RENDER = async function () {
      var ok = await prev();
      if (ok) {
        lastMapAt = new Date().toISOString();
        startStaleWatch(lastMapAt);
        markSectionAge(lastGeneratedAt || lastMapAt);
      } else {
        lastMapAt = null;
      }
    };
    window.SS_SHELL_MAP_RENDER.__ssHooked = true;
  }

  async function loadMapSection() {
    ensureMapRenderHook();
    var qs = new URLSearchParams(location.search);
    var ns = qs.get('namespace');
    var picker = $('map-picker');
    var status = $('map-status');
    if (
      !ns &&
      sessionView &&
      sessionView.scope === 'namespace' &&
      sessionView.namespaces &&
      sessionView.namespaces.length
    ) {
      ns = sessionView.namespaces[0];
      qs.set('namespace', ns);
      history.replaceState(null, '', '/ui/map?' + qs.toString());
    }
    if (!ns) {
      window.SS_SHELL_MAP_NS = '';
      var r = await fetch('/v1/ui/namespaces', { credentials: 'same-origin' });
      if (!r.ok) return;
      var data = await r.json();
      if (picker) {
        picker.hidden = false;
        var html = '<label class="field"><span>Namespace</span><select id="map-ns-pick"><option value="">— choose —</option>';
        (data.namespaces || []).forEach(function (n) {
          html += '<option value="' + esc(n.name) + '">' + esc(n.name) + '</option>';
        });
        html += '</select> <button type="button" id="map-ns-go">Load map</button></label>';
        picker.innerHTML = html;
        var go = $('map-ns-go');
        if (go) {
          go.addEventListener('click', function () {
            var pick = $('map-ns-pick');
            var v = pick && pick.value;
            if (!v) return;
            location.href = '/ui/map?namespace=' + encodeURIComponent(v);
          });
        }
      }
      if (status) {
        status.className = 'banner muted';
        status.innerHTML = '<h2>Select a namespace</h2><p>The cluster map loads only for an allowed namespace.</p>';
      }
      if (window.SS_SHELL_MAP_RENDER) window.SS_SHELL_MAP_RENDER();
      return;
    }
    window.SS_SHELL_MAP_NS = ns;
    if (picker) {
      picker.hidden = false;
      picker.innerHTML = '<p class="muted">namespace: <strong>' + esc(ns) + '</strong> · <a href="/ui/map">change</a></p>';
    }
    if (window.SS_SHELL_MAP_RENDER) await window.SS_SHELL_MAP_RENDER();
  }

  function initBrowseSection() {
    if (window.SS_CONSOLE_INIT) window.SS_CONSOLE_INIT();
  }

  function startStaleWatch(iso) {
    lastGeneratedAt = iso || null;
    if (staleTickTimer) clearInterval(staleTickTimer);
    staleTickTimer = setInterval(function () {
      markSectionAge(lastGeneratedAt);
    }, 1000);
  }

  async function loadSection() {
    var p = path();
    if (
      p === '/ui/monitoring' &&
      sessionView &&
      sessionView.scope === 'namespace' &&
      sessionView.namespaces &&
      sessionView.namespaces.length
    ) {
      var ns0 = sessionView.namespaces[0];
      location.replace('/ui/monitoring/namespaces/' + encodeURIComponent(ns0));
      return;
    }
    if (p.indexOf('/ui/map') === 0) {
      await loadMapSection();
      return;
    }
    if (p.indexOf('/ui/browse') === 0) {
      initBrowseSection();
      return;
    }
    var url = apiPathForSection();
    if (!url) return;
    var r;
    try {
      r = await fetch(url, { credentials: 'same-origin' });
    } catch (e) {
      showSectionReadError(0, {
        message: (e && e.message) || 'network error — read unavailable',
      });
      return;
    }
    if (r.status === 401) {
      var j = await r.json().catch(function () { return {}; });
      if (j.code === 'session_invalid') {
        showLogin(true);
        return;
      }
    }
    if (r.status === 403) {
      var j2 = await r.json().catch(function () { return {}; });
      showRefusal((j2 && j2.message) || 'Forbidden');
      return;
    }
    if (!r.ok) {
      var errBody = await r.json().catch(function () { return {}; });
      showSectionReadError(r.status, errBody);
      return;
    }
    var data = await r.json();
    var p = path();
    var append = pagingAppend;
    pagingAppend = false;
    if (p.indexOf('/ui/overview') === 0) {
      renderOverview(data);
    } else if (p.indexOf('/ui/namespaces') === 0) {
      renderNamespaces(data, p, append);
    } else if (p.indexOf('/ui/nodes') === 0) {
      renderNodes(data, append);
    } else if (p.indexOf('/ui/monitoring') === 0) {
      var nsList = null;
      if (sessionView && sessionView.scope === 'cluster') {
        var r2 = await fetch('/v1/ui/namespaces', { credentials: 'same-origin' });
        if (r2.ok) {
          var d2 = await r2.json();
          nsList = (d2.namespaces || []).map(function (n) {
            return n.name;
          });
        }
      }
      renderMonitoring(data, nsList);
    } else if (p.indexOf('/ui/jobs') === 0) {
      renderJobs(data, p, append);
    } else if (p.indexOf('/ui/audit') === 0) {
      renderAudit(data, append);
    }
    if (data.generated_at) {
      startStaleWatch(data.generated_at);
      markSectionAge(data.generated_at);
    }
  }

  function syncMapStatusHeading(root, mapIso) {
    if (!root || root.id !== 'map-status' || path().indexOf('/ui/map') !== 0) return;
    if (!lastMapAt) return;
    var stale = relAge(mapIso).stale;
    var h2 = root.querySelector('h2');
    if (h2) {
      h2.textContent = stale ? 'Cluster map stale' : 'Cluster live';
    }
    root.className = stale ? 'banner need-login' : 'banner ok';
  }

  function markSectionAge(iso) {
    if (!iso) return;
    sectionAgeRoots().forEach(function (root) {
      var mapIso = lastMapAt;
      var live =
        root.id === 'map-status' && path().indexOf('/ui/map') === 0 && mapIso
          ? relAge(mapIso).stale
            ? 'map stale'
            : 'map live'
          : '';
      if (root.id === 'map-status') syncMapStatusHeading(root, mapIso);
      applyAgeToRoot(root, root.id === 'map-status' && mapIso ? mapIso : iso, live);
    });
  }

  function renderMonitoring(data, nsList) {
    var el = $('section-panel') || $('section-mount');
    if (!el) return;
    var scope = esc(data.scope || 'unknown');
    var ns = data.namespace ? trunc(data.namespace, TRUNC_LEN) : null;
    var html = '<h2>Monitoring</h2><p class="muted">scope: ' + scope;
    if (ns) html += ' · namespace <span' + (ns.title ? ' title="' + ns.title + '"' : '') + '>' + ns.short + '</span>';
    html += '</p>';
    if (data.scope === 'cluster' && nsList && nsList.length) {
      html += '<p class="muted">Per-namespace: ';
      nsList.forEach(function (n, i) {
        var tn = trunc(n, TRUNC_LEN);
        if (i) html += ', ';
        html += '<a href="/ui/monitoring/namespaces/' + encodeURIComponent(n) + '"' + (tn.title ? ' title="' + tn.title + '"' : '') + '>' + tn.short + '</a>';
      });
      html += '</p>';
    }
    if (data.unavailable && data.unavailable.length) {
      html += '<div class="callout"><strong>Unavailable</strong><ul>';
      data.unavailable.forEach(function (u) {
        html += '<li>' + esc(u.name) + ' — ' + esc(u.reason || 'unavailable') + '</li>';
      });
      html += '</ul></div>';
    }
    html += '<ul class="series-list">';
    (data.series || []).forEach(function (s) {
      var nm = trunc(s.name, TRUNC_LEN);
      html += '<li' + (nm.title ? ' title="' + nm.title + '"' : '') + '>' + nm.short + ' ' + esc(JSON.stringify(s.labels)) + ' = ' + esc(s.value != null ? s.value : 'buckets') + '</li>';
    });
    html += '</ul>';
    el.innerHTML = html;
  }

  function renderJobs(data, p, append) {
    var el = $('section-panel') || $('section-mount');
    if (!el) return;
    if (data.id) {
      var pr = data.progress || {};
      var tid = trunc(data.id, TRUNC_LEN);
      var tgt = trunc(data.target, TRUNC_LEN);
      var fail = data.failure ? trunc(data.failure, TRUNC_LEN) : null;
      var html = '<h2>Job <span' + (tid.title ? ' title="' + tid.title + '"' : '') + '>' + tid.short + '</span></h2>';
      html += '<p>' + esc(data.kind) + ' · ' + esc(data.status);
      if (data.paused) html += ' · paused';
      html += ' · target <span' + (tgt.title ? ' title="' + tgt.title + '"' : '') + '>' + tgt.short + '</span></p>';
      html += '<p>progress: ' + esc(JSON.stringify(pr)) + '</p>';
      if (fail) {
        html += '<p class="err">failure: <span' + (fail.title ? ' title="' + fail.title + '"' : '') + '>' + fail.short + '</span></p>';
      }
      if (sessionView && sessionView.affordances && sessionView.affordances.job_control &&
          (data.status === 'running' || data.status === 'starting') && !data.paused) {
        html += '<button type="button" id="job-cancel">Cancel job</button>';
      }
      if (sessionView && sessionView.affordances && sessionView.affordances.job_control &&
          data.status === 'running' && data.paused) {
        html += '<button type="button" id="job-resume">Resume job</button>';
      }
      html += '<p><a href="/ui/jobs">Back</a></p>';
      el.innerHTML = html;
      var jb = $('job-cancel');
      if (jb) {
        jb.addEventListener('click', async function () {
          var msg = 'Cancel job ' + data.id + ' targeting ' + data.target + '?';
          if (!confirmNormal(msg)) return;
          var out = await guardedMutation('Cancel', function () {
            return postJson('/v1/jobs/' + encodeURIComponent(data.id) + '/cancel', {});
          });
          if (out.ok) loadSection();
        });
      }
      var jr = $('job-resume');
      if (jr) {
        jr.addEventListener('click', async function () {
          var msg = 'Resume job ' + data.id + ' targeting ' + data.target + '?';
          if (!confirmNormal(msg)) return;
          var out = await guardedMutation('Resume', function () {
            return postJson('/v1/jobs/' + encodeURIComponent(data.id) + '/resume', {});
          });
          if (out.ok) loadSection();
        });
      }
      return;
    }
    var rows = data.jobs || [];
    var ul = append ? el.querySelector('.job-list') : null;
    var html = '';
    if (!append) {
      html = '<h2>Jobs</h2><ul class="job-list">';
    }
    rows.forEach(function (j) {
      var jid = trunc(j.id, TRUNC_LEN);
      var tgt = trunc(j.target, TRUNC_LEN);
      var st = esc(j.status) + (j.paused ? ' (paused)' : '');
      var pr = j.progress != null ? esc(JSON.stringify(j.progress)) : '—';
      var fail = j.failure ? trunc(j.failure, TRUNC_LEN) : null;
      html += '<li><a href="/ui/jobs/' + encodeURIComponent(j.id) + '"' + (jid.title ? ' title="' + jid.title + '"' : '') + '>' + jid.short + '</a> ';
      html += esc(j.kind) + ' · ' + st + ' · <span' + (tgt.title ? ' title="' + tgt.title + '"' : '') + '>' + tgt.short + '</span>';
      html += ' · progress ' + pr;
      if (fail) {
        html += ' · failure <span class="err"' + (fail.title ? ' title="' + fail.title + '"' : '') + '>' + fail.short + '</span>';
      }
      html += '</li>';
    });
    if (append && ul) {
      ul.insertAdjacentHTML('beforeend', html);
      var oldMore = el.querySelector('.load-more-row');
      if (oldMore) oldMore.remove();
      if (data.next_cursor) {
        ul.insertAdjacentHTML('afterend', bindLoadMore(data.next_cursor));
        wireLoadMore(el);
      }
      return;
    }
    html += '</ul>' + bindLoadMore(data.next_cursor);
    el.innerHTML = html;
    wireLoadMore(el);
  }

  function renderAudit(data, append) {
    var el = $('section-panel');
    if (!el) return;
    var entries = data.entries || [];
    if (!entries.length) {
      el.innerHTML = '<h2>Audit</h2><p class="empty">No audit entries.</p>';
      return;
    }
    var tbody = append ? el.querySelector('tbody') : null;
    var rows = '';
    entries.forEach(function (e) {
      var tm = e.time != null ? esc(e.time) : 'time unavailable';
      var pr = trunc(e.principal || '', TRUNC_LEN);
      var act = trunc(e.action || '', TRUNC_LEN);
      var tgt = trunc(e.target || '', TRUNC_LEN);
      rows += '<tr><td>' + tm + '</td><td' + (pr.title ? ' title="' + pr.title + '"' : '') + '>' + pr.short + '</td>';
      rows += '<td' + (act.title ? ' title="' + act.title + '"' : '') + '>' + act.short + '</td>';
      rows += '<td' + (tgt.title ? ' title="' + tgt.title + '"' : '') + '>' + tgt.short + '</td></tr>';
    });
    if (append && tbody) {
      tbody.insertAdjacentHTML('beforeend', rows);
      var oldMore = el.querySelector('.load-more-row');
      if (oldMore) oldMore.remove();
      if (data.next_cursor) {
        el.insertAdjacentHTML('beforeend', bindLoadMore(data.next_cursor));
        wireLoadMore(el);
      }
      return;
    }
    var html =
      '<h2>Audit</h2><table><thead><tr><th>Time</th><th>Principal</th><th>Action</th><th>Target</th></tr></thead><tbody>' +
      rows +
      '</tbody></table>' +
      bindLoadMore(data.next_cursor);
    el.innerHTML = html;
    wireLoadMore(el);
  }

  function renderNamespaces(data, p, append) {
    var el = $('section-panel');
    if (!el) return;
    el.innerHTML = '<div id="namespaces-panel"></div>';
    var inner = $('namespaces-panel');
    if (data.namespaces && data.namespaces.length === 0) {
      inner.innerHTML = '<p class="empty">No namespaces yet.</p><button type="button" id="ns-create" class="primary">Create namespace</button>';
      bindNamespaceActions(inner);
      return;
    }
    if (data.namespaces) {
      var ul = append ? inner.querySelector('.ns-list') : null;
      var chunk = '';
      data.namespaces.forEach(function (n) {
        var nm = trunc(n.name, TRUNC_LEN);
        var quota = n.quota != null ? esc(JSON.stringify(n.quota)) : 'none';
        chunk += '<li><a href="/ui/namespaces/' + encodeURIComponent(n.name) + '"' + (nm.title ? ' title="' + nm.title + '"' : '') + '>' + nm.short + '</a> — ' + countLabel(n) + ' containers · quota ' + quota + '</li>';
      });
      if (append && ul) {
        ul.insertAdjacentHTML('beforeend', chunk);
        var oldMore = inner.querySelector('.load-more-row');
        if (oldMore) oldMore.remove();
        if (data.next_cursor) {
          inner.insertAdjacentHTML('beforeend', bindLoadMore(data.next_cursor));
          wireLoadMore(inner);
        }
        bindNamespaceActions(inner);
        return;
      }
      var html = '<ul class="ns-list">' + chunk + '</ul>' + bindLoadMore(data.next_cursor);
      inner.innerHTML = html;
      wireLoadMore(inner);
      bindNamespaceActions(inner);
      return;
    }
    if (data.name) {
      var items = (data.containers && data.containers.items) || [];
      var contCursor = data.containers && data.containers.next_cursor;
      var quota = data.quota ? esc(JSON.stringify(data.quota)) : 'none';
      var title = trunc(data.name, TRUNC_LEN);
      var invChunk = '';
      items.forEach(function (c) {
        var cn = trunc(c.name, TRUNC_LEN);
        invChunk += '<li' + (cn.title ? ' title="' + cn.title + '"' : '') + '>' + cn.short + ' (' + esc(c.model) + ')</li>';
      });
      if (append) {
        var inv = inner.querySelector('.container-inv');
        if (inv) {
          inv.insertAdjacentHTML('beforeend', invChunk);
          var oldMore = inner.querySelector('.load-more-row');
          if (oldMore) oldMore.remove();
          if (contCursor) {
            inner.insertAdjacentHTML('beforeend', bindLoadMore(contCursor));
            wireLoadMore(inner);
          }
          bindNamespaceDetail(inner, data);
          return;
        }
      }
      var html2 = '<h2' + (title.title ? ' title="' + title.title + '"' : '') + '>' + title.short + '</h2><p>Quota: ' + quota + '</p><ul class="container-inv">';
      html2 += invChunk + '</ul>' + bindLoadMore(contCursor);
      html2 += '<p><a href="/ui/map?namespace=' + encodeURIComponent(data.name) + '">Cluster map</a> · <a href="/ui/browse?namespace=' + encodeURIComponent(data.name) + '">Browse</a></p>';
      inner.innerHTML = html2;
      wireLoadMore(inner);
      bindNamespaceDetail(inner, data);
    }
  }

  function syncNodesUrl(q, status) {
    var u = new URL(location.href);
    if (q) u.searchParams.set('q', q); else u.searchParams.delete('q');
    if (status) u.searchParams.set('status', status); else u.searchParams.delete('status');
    history.replaceState(null, '', u.pathname + u.search);
  }

  function renderNodes(data, append) {
    var el = $('section-panel');
    if (!el) return;
    var parts = path().split('/');
    var detailId = parts.length === 4 && parts[1] === 'ui' && parts[2] === 'nodes' ? parts[3] : null;
    if (detailId && data.node_name) {
      var unavail = 'unavailable';
      var wt = data.worker_threads != null ? esc(JSON.stringify(data.worker_threads)) : unavail;
      var buf = data.buffers != null ? esc(JSON.stringify(data.buffers)) : unavail;
      var st = data.state != null ? esc(data.state) : unavail;
      var role = data.controller_role != null ? esc(data.controller_role) : unavail;
      var labels = data.labels != null ? esc(JSON.stringify(data.labels)) : unavail;
      var reported = data.reported_by != null ? esc(data.reported_by) : unavail;
      var nm = trunc(data.node_name, TRUNC_LEN);
      el.innerHTML =
        '<div id="nodes-panel"><h2' + (nm.title ? ' title="' + nm.title + '"' : '') + '>' + nm.short + '</h2>' +
        '<p>Readiness: ' + esc(data.readiness) + '</p>' +
        '<p>State: ' + st + '</p>' +
        '<p>Controller role: ' + role + '</p>' +
        '<p>Topology labels: ' + labels + '</p>' +
        '<p>Reported by: ' + reported + '</p>' +
        '<p>Worker threads: ' + wt + '</p>' +
        '<p>Buffers: ' + buf + '</p>' +
        '<p><a href="/ui/nodes">Back to list</a></p></div>';
      return;
    }
    var qs = new URLSearchParams(location.search);
    var qVal = qs.get('q') || '';
    var stVal = qs.get('status') || '';
    var rows = data.nodes || [];
    lastNodesRows = append ? lastNodesRows.concat(rows) : rows;
    var src = esc(data.source || 'unknown');
    var html =
      '<div id="nodes-panel"><p class="muted">source: ' + src + '</p>' +
      '<form id="node-filters" class="inline">' +
      '<label>Search <input type="search" id="node-q" value="' + esc(qVal) + '"></label> ' +
      '<label>Status <select id="node-status"><option value="">any</option>' +
      '<option value="ready">ready</option><option value="not_ready">not_ready</option>' +
      '<option value="pending">pending</option><option value="unknown">unknown</option></select></form>' +
      '<ul class="node-list">';
    var listChunk = '';
    rows.forEach(function (n) {
      var nm = trunc(n.node_name, TRUNC_LEN);
      var role = n.controller_role != null ? esc(n.controller_role) : 'unavailable';
      listChunk += '<li><a href="/ui/nodes/' + encodeURIComponent(n.node_id) + '"' + (nm.title ? ' title="' + nm.title + '"' : '') + '>' + nm.short + '</a> — ' + esc(n.readiness) + ' · role ' + role + '</li>';
    });
    if (append) {
      var ul = el.querySelector('.node-list');
      if (ul) {
        ul.insertAdjacentHTML('beforeend', listChunk);
        var oldMore = el.querySelector('.load-more-row');
        if (oldMore) oldMore.remove();
        if (data.next_cursor) {
          el.insertAdjacentHTML('beforeend', bindLoadMore(data.next_cursor));
          wireLoadMore(el);
        }
        return;
      }
    }
    html += listChunk + '</ul>' + bindLoadMore(data.next_cursor);
    if (sessionView && sessionView.affordances && sessionView.affordances.node_drain) {
      html += '<div id="drain-panel" class="guard-panel">';
      html += '<p class="drain-impact">No new tenant connections or placements; replicas move off before decommission.</p>';
      html += '<label>Node id <input id="drain-target" type="text" value="" autocomplete="off"></label> ';
      html += '<button type="button" id="drain-btn">Drain</button> ';
      html += '<button type="button" id="undrain-btn">Undrain</button></div>';
    }
    if (sessionView && sessionView.affordances && sessionView.affordances.cluster_stop) {
      html += '<button type="button" id="cluster-stop-btn">Stop cluster</button>';
    }
    html += '</div>';
    el.innerHTML = html;
    wireLoadMore(el);
    var sel = $('node-status');
    if (sel) sel.value = stVal;
    var form = $('node-filters');
    if (form) {
      form.addEventListener('submit', function (ev) {
        ev.preventDefault();
        var nq = ($('node-q') && $('node-q').value) || '';
        var ns = ($('node-status') && $('node-status').value) || '';
        syncNodesUrl(nq, ns);
        loadSection();
      });
    }
    var drainBtn = $('drain-btn');
    if (drainBtn) {
      drainBtn.addEventListener('click', async function () {
        var id = ($('drain-target') && $('drain-target').value.trim()) || '';
        var row = lastNodesRows.find(function (n) {
          return n.node_id === id || n.node_name === id;
        });
        var name = row ? row.node_name : '';
        if (!id) return;
        if (!name) {
          showGuardOutcome('unknown node — pick id or name from the list');
          return;
        }
        var msg =
          'Drain ' +
          name +
          ' (' +
          (row ? row.node_id : id) +
          ')? This stops new tenant connections and placements; replicas move off before decommission. Type the node name.';
        if (!confirmStrong(msg, name)) return;
        var nodeId = row ? row.node_id : id;
        var out = await guardedMutation('Drain', function () {
          return adminPost({ op: 'drain', node_id: nodeId });
        });
        if (out.ok) loadSection();
      });
    }
    var undrainBtn = $('undrain-btn');
    if (undrainBtn) {
      undrainBtn.addEventListener('click', async function () {
        var id = ($('drain-target') && $('drain-target').value.trim()) || '';
        var row = lastNodesRows.find(function (n) {
          return n.node_id === id || n.node_name === id;
        });
        var name = row ? row.node_name : '';
        if (!id) return;
        if (!name) {
          showGuardOutcome('unknown node — pick id or name from the list');
          return;
        }
        var msg =
          'Undrain ' +
          name +
          ' (' +
          (row ? row.node_id : id) +
          ')? The node may accept placements again.';
        if (!confirmNormal(msg)) return;
        var nodeId = row ? row.node_id : id;
        var out = await guardedMutation('Undrain', function () {
          return adminPost({ op: 'undrain', node_id: nodeId });
        });
        if (out.ok) loadSection();
      });
    }
    var stopBtn = $('cluster-stop-btn');
    if (stopBtn) {
      stopBtn.addEventListener('click', async function () {
        if (
          !confirmStrong(
            'Stop the cluster? Every service on the cluster will stop. Type cluster to confirm.',
            'cluster'
          )
        )
          return;
        await guardedMutation('Stop', function () { return postJson('/v1/stop', {}); });
      });
    }
  }

  function relAge(iso) {
    if (!iso) return { rel: 'unknown age', abs: '' };
    var t = Date.parse(iso);
    if (isNaN(t)) return { rel: 'unknown age', abs: iso };
    var age = Date.now() - t;
    var stale = age > STALE_MS;
    var rel = stale ? 'stale — ' + Math.round(age / 1000) + 's old' : Math.round(age / 1000) + 's ago';
    return { rel: rel, abs: iso, stale: stale };
  }

  function renderOverview(data) {
    var el = $('overview-panel');
    if (!el) return;
    var src = esc(data.source || 'unknown');
    var ctrl = data.controller || {};
    var ctrlLine = ctrl.available ? 'controller available · raft ' + esc(ctrl.raft || 'unknown') : 'controller unavailable';
    var html = '<p class="panel-age muted"></p>';
    html += '<p class="muted">source: <strong>' + src + '</strong> · ' + ctrlLine + '</p>';
    html += '<dl class="overview-stats">';
    if (data.members) {
      html +=
        '<dt>Members</dt><dd>' +
        esc(data.members.ready) +
        ' ready, ' +
        esc(data.members.not_ready) +
        ' not ready, ' +
        esc(data.members.pending) +
        ' pending, ' +
        esc(data.members.unknown) +
        ' unknown</dd>';
    }
    if (data.counts) {
      html +=
        '<dt>Counts</dt><dd>' +
        esc(data.counts.namespaces) +
        ' namespaces, ' +
        esc(data.counts.containers) +
        (data.counts.container_count_complete ? '' : '+') +
        ' containers</dd>';
    }
    if (data.replicas) {
      html +=
        '<dt>Replicas</dt><dd>' +
        esc(data.replicas.healthy) +
        ' healthy, ' +
        esc(data.replicas.degraded) +
        ' degraded, ' +
        esc(data.replicas.lagging) +
        ' lagging</dd>';
    }
    if (data.jobs) {
      html +=
        '<dt>Jobs</dt><dd>' +
        esc(data.jobs.active) +
        ' active, ' +
        esc(data.jobs.failed) +
        ' failed, ' +
        esc(data.jobs.recently_completed) +
        ' recently completed</dd>';
    }
    html += '</dl>';
    if (data.members && data.members.degraded && data.members.degraded.length) {
      html += '<div class="callout"><strong>Degraded members</strong><ul>';
      data.members.degraded.forEach(function (m) {
        var nm = trunc(m.node_name, TRUNC_LEN);
        html +=
          '<li><span class="status-not_ready"></span><span' +
          (nm.title ? ' title="' + nm.title + '"' : '') +
          '>' +
          nm.short +
          '</span> — ' +
          esc(m.readiness) +
          '</li>';
      });
      html += '</ul></div>';
    }
    if (data.replicas && data.replicas.named && data.replicas.named.length) {
      html += '<div class="callout"><strong>Lagging replicas</strong><ul>';
      data.replicas.named.forEach(function (r) {
        html +=
          '<li>' +
          esc(r.namespace) +
          '/' +
          esc(r.container) +
          ' replica ' +
          esc(r.replica) +
          ' lag ' +
          esc(r.lag_ms) +
          'ms</li>';
      });
      html += '</ul></div>';
    }
    html += '<p class="muted">Readiness: ' + esc((data.readiness && data.readiness.value) || 'unknown') + '</p>';
    el.innerHTML = html;
  }

  function pollableSection() {
    var p = path();
    return (
      p.indexOf('/ui/overview') === 0 ||
      p.indexOf('/ui/monitoring') === 0 ||
      p.indexOf('/ui/jobs') === 0 ||
      p.indexOf('/ui/namespaces') === 0 ||
      p.indexOf('/ui/nodes') === 0 ||
      p.indexOf('/ui/audit') === 0 ||
      p.indexOf('/ui/map') === 0
    );
  }

  function schedulePoll() {
    if (pollTimer) clearInterval(pollTimer);
    pollTimer = setInterval(function () {
      if (pollPaused || document.hidden) return;
      if (pollableSection()) pollOverview();
    }, POLL_MS);
  }

  async function pollOverview() {
    if (overviewInflight) return;
    overviewInflight = true;
    try {
      if (path().indexOf('/ui/map') === 0 && window.SS_SHELL_MAP_RENDER) {
        await window.SS_SHELL_MAP_RENDER();
      } else {
        await loadSection();
      }
    } finally {
      overviewInflight = false;
    }
  }

  function safeNext(next) {
    if (!next || typeof next !== 'string') return false;
    if (next.indexOf('/ui/') !== 0) return false;
    if (next.indexOf('//') >= 0 || next.indexOf('\\') >= 0) return false;
    if (next.indexOf(':') >= 0) return false;
    return true;
  }

  window.SS_SHELL = {
    path: path,
    cookieMutationHeaders: cookieMutationHeaders,
    confirmStrong: confirmStrong,
    confirmNormal: confirmNormal,
    guardedMutation: guardedMutation,
    postJson: postJson,
    adminPost: adminPost,
    probeSession: async function () {
      var r = await fetch('/v1/auth/session', { credentials: 'same-origin' });
      if (!r.ok) {
        showLogin(false);
        return false;
      }
      var j = await r.json();
      attachSession(j);
      return true;
    },
    login: async function (adminToken) {
      var r = await fetch('/v1/auth/login', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json', Authorization: 'Bearer ' + adminToken },
        body: '{}',
      });
      var j = await r.json();
      if (!r.ok) return { ok: false, message: (j && j.message) || 'login failed' };
      var ok = await window.SS_SHELL.probeSession();
      if (!ok) return { ok: false, message: 'session probe failed' };
      var params = new URLSearchParams(location.search);
      var next = params.get('next');
      if (safeNext(next)) location.href = next;
      return { ok: true };
    },
    logout: async function () {
      await fetch('/v1/auth/logout', {
        method: 'POST',
        credentials: 'same-origin',
        headers: cookieMutationHeaders(),
      });
      showLogin(false);
    },
    setPollPaused: function (v) {
      pollPaused = !!v;
    },
  };

  document.addEventListener('visibilitychange', function () {
    if (!document.hidden) {
      if (lastGeneratedAt) markSectionAge(lastGeneratedAt);
      pollOverview();
    }
  });

  document.addEventListener('DOMContentLoaded', function () {
    var pause = $('poll-pause');
    if (pause) {
      pause.addEventListener('click', function () {
        pollPaused = !pollPaused;
        pause.setAttribute('aria-pressed', pollPaused ? 'true' : 'false');
        pause.textContent = pollPaused ? 'Resume refresh' : 'Pause refresh';
        if (pollPaused && lastGeneratedAt) markSectionAge(lastGeneratedAt);
      });
    }
    var loginBtn = $('login');
    if (loginBtn) {
      loginBtn.addEventListener('click', async function () {
        var tok = $('admin-tok');
        var status = $('login-status');
        var v = tok && tok.value.trim();
        if (!v) {
          if (status) status.textContent = 'paste admin.token first';
          return;
        }
        if (status) status.textContent = 'signing in…';
        var out = await window.SS_SHELL.login(v);
        if (status) status.textContent = out.ok ? 'signed in' : out.message || 'failed';
      });
    }
    var outBtn = $('logout');
    if (outBtn) outBtn.addEventListener('click', function () {
      window.SS_SHELL.logout();
    });
    var reloadBtn = $('config-reload');
    if (reloadBtn) {
      reloadBtn.addEventListener('click', async function () {
        if (!confirmNormal('Reload configuration from disk?')) return;
        await guardedMutation('Reload', function () { return postJson('/v1/reload', {}); });
      });
    }
    window.SS_SHELL.probeSession();
  });
})();