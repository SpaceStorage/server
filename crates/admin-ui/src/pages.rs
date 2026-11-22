//! HTML pages (maud) for cluster map, console, and the 018 admin shell.

use crate::shell::ShellSection;
use maud::{html, Markup, DOCTYPE, PreEscaped};

pub const SHELL_JS: &str = include_str!("assets/shell.js");
pub const SHELL_CSS: &str = include_str!("assets/shell.css");

/// Shared console browse panels (`009`) for standalone and shell-hosted browse.
fn console_panels() -> Markup {
    html! {
        section class="panel" {
            h2 { "Create container" }
            p class="hint" { "Creates a typed container in a namespace (cluster admin)." }
            label class="field" {
                span { "Namespace" }
                input id="ns" type="text" value="acme" autocomplete="off";
            }
            label class="field" {
                span { "Container name" }
                input id="name" type="text" value="events" autocomplete="off";
            }
            label class="field" {
                span { "Type" }
                input id="ty" type="text" value="log_stream" autocomplete="off";
            }
            div class="row" {
                button id="create" type="button" { "Create" }
            }
        }
        section class="panel" {
            h2 { "Browse rows" }
            p class="hint" { "Reads from the namespace/container fields above." }
            div class="row" {
                button id="browse" type="button" { "Query" }
            }
            pre id="out" {}
        }
        section class="panel" {
            h2 { "Configuration" }
            label class="field" {
                span { "Scope" }
                select id="cfg-scope" {
                    option value="namespace" selected { "namespace" }
                    option value="cluster" { "cluster" }
                }
            }
            label class="field" {
                span { "Settings JSON" }
                textarea id="cfg-settings" rows="4" { "{\"labels\":{\"env\":\"dev\"}}" }
            }
            div class="row" {
                button id="cfg-load" class="secondary" type="button" { "Load" }
                button id="cfg-save" type="button" { "Apply" }
            }
            pre id="cfg-out" {}
        }
    }
}

fn shell_head(title: &str, extra_css: Option<&str>) -> Markup {
    html! {
        head {
            meta charset="utf-8";
            meta name="viewport" content="width=device-width, initial-scale=1";
            title { (title) }
            link rel="stylesheet" href="/ui/assets/ui.css";
            @if let Some(href) = extra_css {
                link rel="stylesheet" href=(href);
            }
        }
    }
}

fn app_header(active: &str) -> Markup {
    html! {
        header class="app" {
            span class="brand" { "SpaceStorage" }
            nav {
                a href="/ui/cluster" class=(if active == "cluster" { "active" } else { "" }) { "Cluster" }
                a href="/ui/console" class=(if active == "console" { "active" } else { "" }) { "Console" }
            }
        }
    }
}

/// Login: admin token → SessionToken via `/v1/auth/login`.
fn session_block() -> Markup {
    html! {
        section class="panel" id="login-panel" {
            h2 { "Log in" }
            p class="hint" {
                "Use the contents of "
                code { "admin.token" }
                ". Docker lab: "
                code { "docker compose exec db-a cat /etc/spacestorage/secrets/admin.token" }
            }
            div class="row" {
                input id="admin-tok" type="password" placeholder="paste admin.token here" autocomplete="off";
                button id="login" type="button" { "Log in" }
                span id="login-status" class="muted" {}
            }
            details {
                summary { "Advanced: paste session token" }
                div class="row" {
                    input id="tok" type="password" placeholder="SessionToken from /v1/auth/login" autocomplete="off";
                    button id="save-tok" class="secondary" type="button" { "Save session" }
                }
            }
        }
    }
}

fn session_script() -> PreEscaped<&'static str> {
    PreEscaped(
        r#"(function(){
  function save(t){try{sessionStorage.setItem('ss_token',t);}catch(e){}}
  function hasSession(){try{return!!sessionStorage.getItem('ss_token');}catch(e){return false;}}
  var panel=document.getElementById('login-panel');
  if(panel&&hasSession()){
    panel.querySelector('h2').textContent='Session';
    var hint=panel.querySelector('.hint');
    if(hint)hint.textContent='You are logged in. API calls use the saved session token.';
  }
  var saveBtn=document.getElementById('save-tok');
  if(saveBtn)saveBtn.onclick=function(){
    var v=document.getElementById('tok').value.trim();
    save(v);location.reload();
  };
  var loginBtn=document.getElementById('login');
  if(loginBtn)loginBtn.onclick=async function(){
    var admin=document.getElementById('admin-tok').value.trim();
    var status=document.getElementById('login-status');
    if(!admin){if(status)status.textContent='paste admin.token first';return;}
    if(status)status.textContent='signing in…';
    try{
      var r=await fetch('/v1/auth/login',{
        method:'POST',
        headers:{'Content-Type':'application/json','Authorization':'Bearer '+admin},
        body:'{}'
      });
      var j=await r.json();
      if(!r.ok){if(status)status.textContent=(j&&j.message)||('HTTP '+r.status);return;}
      save(j.token||'');
      if(status)status.textContent='ok';
      location.reload();
    }catch(e){if(status)status.textContent=String(e);}
  };
})();"#,
    )
}

pub fn cluster_map_html(interval_secs: u32) -> String {
    let markup: Markup = html! {
        (DOCTYPE)
        html {
            (shell_head("SpaceStorage — Cluster", None))
            body {
                (app_header("cluster"))
                main {
                    div id="map-status" class="banner need-login" {
                        h2 { "Loading…" }
                    }
                    (session_block())
                    div id="map" {}
                    p class="muted" { "Refreshes every " (interval_secs) "s while this tab is open." }
                }
                script src="/ui/assets/cluster-map.js" {}
                script {
                    (format!("window.SS_MAP_INTERVAL={interval_secs};"))
                    (session_script())
                }
            }
        }
    };
    markup.into_string()
}

fn shell_nav(active: ShellSection) -> Markup {
    let links = [
        (ShellSection::Overview, "/ui/overview", None),
        (ShellSection::Namespaces, "/ui/namespaces", None),
        (ShellSection::Nodes, "/ui/nodes", None),
        (ShellSection::Monitoring, "/ui/monitoring", Some("metrics_read")),
        (ShellSection::Jobs, "/ui/jobs", None),
        (ShellSection::Audit, "/ui/audit", Some("audit_read")),
        (ShellSection::Map, "/ui/map", Some("map")),
        (ShellSection::Browse, "/ui/browse", Some("browse")),
    ];
    html! {
        nav class="shell-nav" hidden {
            @for (sec, href, aff) in links {
                @if let Some(key) = aff {
                    a href=(href) class=(if sec == active { "active" } else { "" }) data-aff=(key) { (sec.label()) }
                } @else {
                    a href=(href) class=(if sec == active { "active" } else { "" }) { (sec.label()) }
                }
            }
        }
    }
}

/// Authenticated or login-only shell document (`contracts/shell-http.md`).
pub fn shell_html(data_path: &str, section: ShellSection) -> String {
    let markup: Markup = html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (section.label()) " — SpaceStorage" }
                link rel="stylesheet" href="/ui/assets/shell.css";
                link rel="stylesheet" href="/ui/assets/ui.css";
            }
            body data-path=(data_path) {
                header class="app" role="banner" {
                    span class="brand" { "SpaceStorage" }
                    span id="principal-label" class="muted" {}
                    span id="scope-label" class="muted" {}
                    span id="session-expiry" class="muted" {}
                    span id="guard-outcome" class="muted" role="status" {}
                    button id="config-reload" type="button" class="secondary" hidden { "Reload config" }
                    button id="logout" type="button" class="secondary" { "Sign out" }
                }
                (shell_nav(section))
                main role="main" {
                    noscript {
                        p { "This console has limited interactivity without JavaScript. Sign-in and navigation require the shell script." }
                    }
                    p id="section-refusal" class="panel err" hidden {}
                    section class="panel" id="login-panel" {
                        h2 { "Log in" }
                        p class="hint" {
                            "Use "
                            code { "admin.token" }
                            " with the button below. Session uses an HttpOnly cookie."
                        }
                        label class="field" {
                            span { "Admin token" }
                            input id="admin-tok" type="password" autocomplete="off";
                        }
                        div class="row" {
                            button id="login" type="button" { "Sign in" }
                            span id="login-status" class="muted" {}
                        }
                    }
                    section class="panel" id="section-mount" hidden {
                        h1 { (section.label()) }
                        @if section == ShellSection::Overview {
                            div class="row" {
                                button id="poll-pause" type="button" class="secondary" aria-pressed="false" { "Pause refresh" }
                            }
                            div id="overview-panel" {}
                        } @else if section == ShellSection::Map {
                            div class="row" {
                                button id="poll-pause" type="button" class="secondary" aria-pressed="false" { "Pause refresh" }
                            }
                            div id="map-picker" class="row" hidden {}
                            div id="map-mount" class="map-mount" {}
                            div id="map" {}
                            div id="map-status" class="banner" {}
                            script src="/ui/assets/cluster-map.js" {}
                        } @else if section == ShellSection::Browse {
                            link rel="stylesheet" href="/ui/assets/console.css";
                            div id="browse-mount" class="browse-mount" {
                                (console_panels())
                            }
                            div id="console-status" class="banner" {}
                            script src="/ui/assets/console.js" {}
                        } @else if section == ShellSection::Jobs || section == ShellSection::Monitoring {
                            div class="row" {
                                button id="poll-pause" type="button" class="secondary" aria-pressed="false" { "Pause refresh" }
                            }
                            div id="section-panel" {}
                        } @else if section == ShellSection::Namespaces || section == ShellSection::Nodes || section == ShellSection::Audit {
                            div class="row" {
                                button id="poll-pause" type="button" class="secondary" aria-pressed="false" { "Pause refresh" }
                            }
                            div id="section-panel" {}
                            p class="muted" { "Data for this section loads after session probe succeeds." }
                        } @else {
                            p class="muted" { "Data for this section loads after session probe succeeds." }
                        }
                    }
                }
                script src="/ui/assets/shell.js" {}
            }
        }
    };
    markup.into_string()
}

pub fn console_html() -> String {
    let markup: Markup = html! {
        (DOCTYPE)
        html {
            (shell_head("SpaceStorage — Console", Some("/ui/assets/console.css")))
            body {
                (app_header("console"))
                main {
                    div id="console-status" class="banner need-login" {
                        h2 { "Loading…" }
                    }
                    (session_block())
                    (console_panels())
                }
                script src="/ui/assets/console.js" {}
                script { (session_script()) }
            }
        }
    };
    markup.into_string()
}
