//! S3 XML response builders (019).

/// S3 DNS-style bucket name (`002` / FR-019).
pub fn s3_bucket_name_valid(name: &str) -> bool {
    let len = name.len();
    if len < 3 || len > 63 {
        return false;
    }
    let bytes = name.as_bytes();
    let label = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let middle = |b: u8| label(b) || b == b'.' || b == b'-';
    label(bytes[0]) && label(bytes[len - 1]) && bytes[1..len - 1].iter().all(|&b| middle(b))
}

/// ListBuckets `Name` element: DNS-valid names as-is; others as `x--<hex>`.
pub fn s3_bucket_list_name(canonical: &str) -> String {
    if s3_bucket_name_valid(canonical) {
        canonical.to_string()
    } else {
        format!("x--{}", hex::encode(canonical.as_bytes()))
    }
}

pub fn list_buckets_xml(buckets: &[(String, String)]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Buckets>",
    );
    for (name, ty) in buckets {
        xml.push_str(&format!(
            "<Bucket><Name>{}</Name><CreationDate>1970-01-01T00:00:00.000Z</CreationDate><SpaceStorageType>{}</SpaceStorageType></Bucket>",
            xml_escape(name),
            xml_escape(ty)
        ));
    }
    xml.push_str("</Buckets></ListAllMyBucketsResult>");
    xml
}

pub fn list_objects_v2_xml(
    bucket: &str,
    keys: &[(String, String, u64, u64)],
    prefixes: &[String],
    truncated: bool,
    continuation_token: Option<&str>,
    url_encode: bool,
    legacy_v1: bool,
) -> String {
    let enc = |s: &str| {
        if url_encode {
            url_encode_key(s)
        } else {
            xml_escape(s)
        }
    };
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult><Name>{}</Name>",
        xml_escape(bucket)
    );
    if url_encode {
        xml.push_str("<EncodingType>url</EncodingType>");
    }
    if let Some(tok) = continuation_token {
        let tag = if legacy_v1 {
            "NextMarker"
        } else {
            "NextContinuationToken"
        };
        xml.push_str(&format!("<{tag}>{}</{tag}>", enc(tok)));
    }
    xml.push_str(&format!("<IsTruncated>{truncated}</IsTruncated>"));
    for (k, etag, size, modified) in keys {
        xml.push_str(&format!(
            "<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>{}</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            enc(k),
            crate::meta::iso8601(*modified),
            xml_escape(etag),
            size
        ));
    }
    for p in prefixes {
        xml.push_str(&format!(
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            enc(p)
        ));
    }
    xml.push_str("</ListBucketResult>");
    xml
}

fn url_encode_key(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn bucket_location_xml(region: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><LocationConstraint>{}</LocationConstraint>",
        xml_escape(region)
    )
}

pub fn list_parts_xml(upload_id: &str, parts: &[(u32, String)]) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListPartsResult><UploadId>{}</UploadId><Parts>",
        xml_escape(upload_id)
    );
    for (n, etag) in parts {
        xml.push_str(&format!(
            "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
            xml_escape(etag)
        ));
    }
    xml.push_str("</Parts></ListPartsResult>");
    xml
}

pub fn list_multipart_uploads_xml(bucket: &str, uploads: &[(String, String, String)]) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListMultipartUploadsResult><Bucket>{}</Bucket><Uploads>",
        xml_escape(bucket)
    );
    for (key, upload_id, _) in uploads {
        xml.push_str(&format!(
            "<Upload><Key>{}</Key><UploadId>{}</UploadId></Upload>",
            xml_escape(key),
            xml_escape(upload_id)
        ));
    }
    xml.push_str("</Uploads></ListMultipartUploadsResult>");
    xml
}

pub fn delete_objects_result_xml(
    deleted: &[&str],
    errors: &[(String, String, String)],
    quiet: bool,
) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><DeleteResult>");
    if !quiet && !deleted.is_empty() {
        xml.push_str("<Deleted>");
        for k in deleted {
            xml.push_str(&format!("<Key>{}</Key>", xml_escape(k)));
        }
        xml.push_str("</Deleted>");
    }
    for (key, code, message) in errors {
        xml.push_str(&format!(
            "<Error><Key>{}</Key><Code>{}</Code><Message>{}</Message></Error>",
            xml_escape(key),
            xml_escape(code),
            xml_escape(message),
        ));
    }
    xml.push_str("</DeleteResult>");
    xml
}

pub fn parse_delete_keys(body: &str) -> (Vec<String>, bool) {
    let quiet = body.contains("<Quiet>true</Quiet>") || body.contains("<Quiet>1</Quiet>");
    let mut keys = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<Key>") {
        let after = &rest[start + 5..];
        if let Some(end) = after.find("</Key>") {
            keys.push(after[..end].to_string());
            rest = &after[end + 6..];
        } else {
            break;
        }
    }
    (keys, quiet)
}

pub fn initiate_multipart_xml(upload_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult><UploadId>{}</UploadId></InitiateMultipartUploadResult>",
        xml_escape(upload_id)
    )
}

pub fn complete_multipart_upload_result(bucket: &str, key: &str, etag: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUploadResult><Location>/{loc}</Location><Bucket>{}</Bucket><Key>{}</Key><ETag>{}</ETag></CompleteMultipartUploadResult>",
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(etag),
        loc = format!("{}/{}", xml_escape(bucket), xml_escape(key)),
    )
}

/// `(part_number, etag)` in document order.
pub fn parse_complete_multipart_parts(body: &str) -> Option<Vec<(u32, String)>> {
    if !body.contains("<CompleteMultipartUpload") {
        return None;
    }
    let mut parts = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<Part>") {
        let chunk = &rest[start + 6..];
        let end = chunk.find("</Part>")?;
        let part_xml = &chunk[..end];
        let number = part_xml
            .split("<PartNumber>")
            .nth(1)?
            .split("</PartNumber>")
            .next()?
            .trim()
            .parse()
            .ok()?;
        let etag = part_xml
            .split("<ETag>")
            .nth(1)?
            .split("</ETag>")
            .next()?
            .trim()
            .to_string();
        parts.push((number, etag));
        rest = &chunk[end + 7..];
    }
    if parts.is_empty() { None } else { Some(parts) }
}

pub fn error_xml(code: &str, message: &str, resource: &str, request_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{}</Code><Message>{}</Message><Resource>{}</Resource><RequestId>{}</RequestId></Error>",
        xml_escape(code),
        xml_escape(message),
        xml_escape(resource),
        xml_escape(request_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_complete_multipart_parts_order() {
        let body = r#"<?xml version="1.0"?><CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>"a"</ETag></Part><Part><PartNumber>2</PartNumber><ETag>"b"</ETag></Part></CompleteMultipartUpload>"#;
        let parts = parse_complete_multipart_parts(body).expect("parse");
        assert_eq!(parts, vec![(1, "\"a\"".into()), (2, "\"b\"".into())]);
    }

    #[test]
    fn s3_bucket_name_validation() {
        assert!(s3_bucket_name_valid("my-bucket.1"));
        assert!(s3_bucket_name_valid("crud"));
        assert!(!s3_bucket_name_valid("b"));
        assert!(!s3_bucket_name_valid("MP"));
        assert!(!s3_bucket_name_valid("a".repeat(64).as_str()));
    }

    #[test]
    fn s3_bucket_list_name_escaping() {
        assert_eq!(s3_bucket_list_name("ok-bkt"), "ok-bkt");
        assert_eq!(s3_bucket_list_name("MP"), "x--4d50");
    }
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}
