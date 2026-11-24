//! HTTP method/path/query → classified S3 verb (019 US1).

use std::collections::HashMap;

/// Resolved routing decision before `compat::classify` gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedOp {
    pub verb: String,
    pub args: Vec<String>,
}

/// Map HTTP request to dispatch verb + string args (bucket, key, prefixes, upload ids).
pub fn route_http(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
) -> Result<RoutedOp, &'static str> {
    let method = method.to_ascii_uppercase();
    let (bucket, key) = split_bucket_key(path);

    match method.as_str() {
        "GET" if bucket.is_none() => Ok(RoutedOp {
            verb: "LISTBUCKETS".into(),
            args: vec![],
        }),
        "GET"
            if bucket.is_some()
                && key.is_none()
                && query.get("list-type").map(String::as_str) == Some("2") =>
        {
            Ok(RoutedOp {
                verb: "LISTOBJECTSV2".into(),
                args: vec![
                    bucket.unwrap(),
                    query.get("prefix").cloned().unwrap_or_default(),
                    query.get("max-keys").cloned().unwrap_or_default(),
                    query.get("continuation-token").cloned().unwrap_or_default(),
                    query.get("delimiter").cloned().unwrap_or_default(),
                    query.get("encoding-type").cloned().unwrap_or_default(),
                    query.get("start-after").cloned().unwrap_or_default(),
                ],
            })
        }
        "GET"
            if bucket.is_some()
                && key.is_none()
                && !query.contains_key("location")
                && !query.contains_key("uploads") =>
        {
            Ok(RoutedOp {
                verb: "LISTOBJECTS".into(),
                args: vec![
                    bucket.unwrap(),
                    query.get("prefix").cloned().unwrap_or_default(),
                    query.get("marker").cloned().unwrap_or_default(),
                    query.get("max-keys").cloned().unwrap_or_default(),
                    query.get("delimiter").cloned().unwrap_or_default(),
                    query.get("encoding-type").cloned().unwrap_or_default(),
                ],
            })
        }
        "GET" if bucket.is_some() && key.is_none() && query.contains_key("location") => {
            Ok(RoutedOp {
                verb: "GETBUCKETLOCATION".into(),
                args: vec![bucket.unwrap()],
            })
        }
        "GET" if bucket.is_some() && key.is_none() && query.contains_key("uploads") => {
            Ok(RoutedOp {
                verb: "LISTMULTIPARTUPLOADS".into(),
                args: vec![bucket.unwrap()],
            })
        }
        "GET" if query.contains_key("uploadId") && key.is_some() => Ok(RoutedOp {
            verb: "LISTPARTS".into(),
            args: vec![
                bucket.unwrap_or_default(),
                key.unwrap_or_default(),
                query.get("uploadId").cloned().unwrap_or_default(),
            ],
        }),
        "GET" if key.is_some() => Ok(RoutedOp {
            verb: "GETOBJECT".into(),
            args: vec![bucket.unwrap_or_default(), key.unwrap()],
        }),
        "HEAD" if bucket.is_some() && key.is_none() => Ok(RoutedOp {
            verb: "HEADBUCKET".into(),
            args: vec![bucket.unwrap()],
        }),
        "HEAD" if key.is_some() => Ok(RoutedOp {
            verb: "HEADOBJECT".into(),
            args: vec![bucket.unwrap_or_default(), key.unwrap()],
        }),
        "PUT" if bucket.is_some() && key.is_none() => Ok(RoutedOp {
            verb: "CREATEBUCKET".into(),
            args: vec![bucket.unwrap()],
        }),
        "PUT" if query.contains_key("uploadId") && query.contains_key("partNumber") => {
            Ok(RoutedOp {
                verb: "UPLOADPART".into(),
                args: vec![query.get("uploadId").cloned().unwrap_or_default()],
            })
        }
        "PUT" if key.is_some() => Ok(RoutedOp {
            verb: "PUTOBJECT".into(),
            args: vec![bucket.unwrap_or_default(), key.unwrap()],
        }),
        "DELETE"
            if query.contains_key("uploadId") && key.as_deref().is_some_and(|k| !k.is_empty()) =>
        {
            Ok(RoutedOp {
                verb: "ABORTMULTIPARTUPLOAD".into(),
                args: vec![query.get("uploadId").cloned().unwrap_or_default()],
            })
        }
        "DELETE" if bucket.is_some() && key.is_none() && query.contains_key("uploadId") => {
            Ok(RoutedOp {
                verb: "ABORTMULTIPARTUPLOAD".into(),
                args: vec![query.get("uploadId").cloned().unwrap_or_default()],
            })
        }
        "DELETE" if bucket.is_some() && key.is_none() => Ok(RoutedOp {
            verb: "DELETEBUCKET".into(),
            args: vec![bucket.unwrap()],
        }),
        "DELETE" if key.is_some() => Ok(RoutedOp {
            verb: "DELETEOBJECT".into(),
            args: vec![bucket.unwrap_or_default(), key.unwrap()],
        }),
        "POST" if query.contains_key("delete") => Ok(RoutedOp {
            verb: "DELETEOBJECTS".into(),
            args: vec![bucket.unwrap_or_default()],
        }),
        "POST" if query.contains_key("uploads") => Ok(RoutedOp {
            verb: "CREATEMULTIPARTUPLOAD".into(),
            args: vec![bucket.unwrap_or_default(), key.unwrap_or_default()],
        }),
        "POST" if query.contains_key("uploadId") => Ok(RoutedOp {
            verb: "COMPLETEMULTIPARTUPLOAD".into(),
            args: vec![query.get("uploadId").cloned().unwrap_or_default()],
        }),
        _ => Err("unrouted"),
    }
}

/// Subresource probes routed to classify MustNot (versioning, acl, …).
pub fn route_subresource(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
) -> Option<RoutedOp> {
    let method = method.to_ascii_uppercase();
    let (bucket, _) = split_bucket_key(path);
    let b = bucket.unwrap_or_else(|| "bucket".into());
    if query.contains_key("versioning") || (method == "PUT" && query.contains_key("versioning")) {
        return Some(RoutedOp {
            verb: "VERSIONING".into(),
            args: vec![b],
        });
    }
    if query.contains_key("acl") {
        return Some(RoutedOp {
            verb: "ACL".into(),
            args: vec![b],
        });
    }
    if query.contains_key("object-lock") {
        return Some(RoutedOp {
            verb: "OBJECTLOCK".into(),
            args: vec![b],
        });
    }
    if query.contains_key("replication") {
        return Some(RoutedOp {
            verb: "REPLICATION".into(),
            args: vec![b],
        });
    }
    if query.contains_key("tagging") {
        return Some(RoutedOp {
            verb: "TAGGING".into(),
            args: vec![b],
        });
    }
    if query.contains_key("lifecycle") {
        return Some(RoutedOp {
            verb: "LIFECYCLE".into(),
            args: vec![b],
        });
    }
    if query.contains_key("cors") {
        return Some(RoutedOp {
            verb: "CORS".into(),
            args: vec![b],
        });
    }
    if query.contains_key("website") {
        return Some(RoutedOp {
            verb: "WEBSITE".into(),
            args: vec![b],
        });
    }
    if query.contains_key("policy") {
        return Some(RoutedOp {
            verb: "POLICY".into(),
            args: vec![b],
        });
    }
    if query.contains_key("select") {
        return Some(RoutedOp {
            verb: "SELECT".into(),
            args: vec![b],
        });
    }
    if query.contains_key("legal-hold") {
        return Some(RoutedOp {
            verb: "OBJECTLOCK".into(),
            args: vec![b],
        });
    }
    if query.contains_key("attributes") {
        return Some(RoutedOp {
            verb: "GETOBJECTATTRIBUTES".into(),
            args: vec![b],
        });
    }
    None
}

pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parts_with_upload_id_query() {
        let mut q = HashMap::new();
        q.insert("uploadId".into(), "up-1".into());
        let r = route_http("GET", "/b/k", &q).expect("route");
        assert_eq!(r.verb, "LISTPARTS");
    }
}

fn split_bucket_key(path: &str) -> (Option<String>, Option<String>) {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return (None, None);
    }
    match path.split_once('/') {
        None => (Some(percent_decode(path)), None),
        Some((bucket, rest)) if rest.is_empty() => (Some(percent_decode(bucket)), None),
        Some((bucket, key)) => (Some(percent_decode(bucket)), Some(percent_decode(key))),
    }
}
