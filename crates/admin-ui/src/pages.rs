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
                script src="/ui/assets/cluster-map.js" {}
                script { (format!("window.SS_MAP_INTERVAL={interval_secs};")) }
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
                script src="/ui/assets/console.js" {}
            }
        }
    };
    markup.into_string()
}
