//! HTML pages (maud) for cluster map and console.

use maud::{html, Markup, DOCTYPE};

pub fn cluster_map_html(interval_secs: u32) -> String {
    let markup: Markup = html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                title { "SpaceStorage — Cluster map" }
                style { "body{font-family:system-ui,sans-serif;margin:1.5rem} #map{white-space:pre-wrap;font-family:ui-monospace,monospace}" }
            }
            body {
                h1 { "Cluster map" }
                p { "Poll interval: " (interval_secs) "s" }
                div id="map" { "Loading…" }
                p {
                    "Session token (Bearer): "
                    input id="tok" type="password" placeholder="paste token from /v1/auth/login";
                    button id="save-tok" type="button" { "Save" }
                }
                script src="/ui/assets/cluster-map.js" {}
                script {
                    (format!("window.SS_MAP_INTERVAL={interval_secs};"))
                    "document.getElementById('save-tok').onclick=function(){try{sessionStorage.setItem('ss_token',document.getElementById('tok').value.trim());}catch(e){}location.reload();};"
                }
            }
        }
    };
    markup.into_string()
}

pub fn console_html() -> String {
    let markup: Markup = html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                title { "SpaceStorage — Console" }
                link rel="stylesheet" href="/ui/assets/console.css";
            }
            body {
                h1 { "Namespace console" }
                section {
                    h2 { "Create container" }
                    label { "namespace " input id="ns" type="text" value="acme"; }
                    label { "name " input id="name" type="text" value="events"; }
                    label { "type " input id="ty" type="text" value="log_stream"; }
                    button id="create" { "Create" }
                }
                section {
                    h2 { "Browse" }
                    button id="browse" { "Query" }
                    pre id="out" {}
                }
                section {
                    h2 { "Configuration" }
                    label {
                        "scope "
                        select id="cfg-scope" {
                            option value="namespace" selected { "namespace" }
                            option value="cluster" { "cluster" }
                        }
                    }
                    label { "settings JSON " textarea id="cfg-settings" rows="4" { "{\"labels\":{\"env\":\"dev\"}}" } }
                    button id="cfg-load" type="button" { "Load" }
                    button id="cfg-save" type="button" { "Apply" }
                    pre id="cfg-out" {}
                }
                section {
                    h2 { "Session" }
                    label {
                        "Bearer token "
                        input id="tok" type="password" placeholder="from /v1/auth/login";
                    }
                    button id="save-tok" type="button" { "Save token" }
                }
                script src="/ui/assets/console.js" {}
                script {
                    "document.getElementById('save-tok').onclick=function(){try{sessionStorage.setItem('ss_token',document.getElementById('tok').value.trim());}catch(e){}};"
                }
            }
        }
    };
    markup.into_string()
}
