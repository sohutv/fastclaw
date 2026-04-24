use chrono::Utc;
use itertools::Itertools;
use sha2::{Digest, Sha256};

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct AuthHeader {
    pub x_date: String,
    pub host: String,
    pub auth: String,
    pub canonical_request: String,
    pub string_to_sign: String,
}
pub fn create_auth_header(
    http_method: reqwest::Method,
    request_url: &url::Url,
    body: Option<&[u8]>,
    request_datetime: chrono::DateTime<Utc>,
    region: &str,
    service_name: &str,
    access_key: &str,
    secret_key: &str,
) -> crate::Result<AuthHeader> {
    let (
        x_date,
        host,
        credential_scope,
        signed_headers,
        signature,
        canonical_request,
        string_to_sign,
    ) = signature(
        http_method,
        request_url,
        body,
        request_datetime,
        region,
        service_name,
        secret_key.as_bytes(),
    )?;
    Ok(AuthHeader {
        x_date,
        host,
        auth: format!(
            "HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
        ),
        canonical_request,
        string_to_sign,
    })
}
fn signature(
    http_method: reqwest::Method,
    request_url: &url::Url,
    body: Option<&[u8]>,
    request_datetime: chrono::DateTime<Utc>,
    region: &str,
    service_name: &str,
    secret_key: &[u8],
) -> crate::Result<(String, String, String, String, String, String, String)> {
    let canonical_query_string = {
        let sorted_query_pairs = request_url
            .query_pairs()
            .into_iter()
            .sorted_by_key(|(name, _)| name.to_string())
            .collect_vec();
        let mut encoded_query = url::form_urlencoded::Serializer::new(String::new());
        for (name, val) in sorted_query_pairs {
            encoded_query.append_pair(name.as_ref(), val.as_ref());
        }
        format!("{}", encoded_query.finish().as_str())
    };
    let (canonical_headers, signed_headers, x_date, host) = {
        let x_date = request_datetime.format("%Y%m%dT%H%M%SZ").to_string();
        let host = request_url.host().map(|it| it.to_string()).expect(&format!(
            "unexpected url without host, url: {}",
            request_url.to_string()
        ));
        let headers = vec![("Host", host.clone()), ("X-Date", x_date.clone())];
        let mut canonical_headers = vec![];
        let mut signed_headers = vec![];
        for (name, val) in headers {
            let name = name.to_lowercase();
            canonical_headers.push(format!("{}:{}\n", name, val.trim()));
            signed_headers.push(name);
        }
        (
            canonical_headers.join(""),
            signed_headers.join(";"),
            x_date,
            host,
        )
    };
    let body_hash = if let Some(body) = body {
        hex::encode(sha2::Sha256::digest(body))
    } else {
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string()
    };

    let canonical_request = format!(
        r#"{}
{}
{}
{}
{}
{}"#,
        http_method.as_str(),
        request_url.path(),
        canonical_query_string,
        canonical_headers,
        signed_headers,
        body_hash
    );
    let credential_scope_date = request_datetime.format("%Y%m%d").to_string();
    let credential_scope = format!(
        "{}/{}/{}/request",
        credential_scope_date, region, service_name,
    );
    let string_to_sign = format!(
        r#"HMAC-SHA256
{x_date}
{credential_scope}
{}"#,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let signature_string = {
        use ring::hmac::{HMAC_SHA256, Key};

        let k_date = ring::hmac::sign(
            &Key::new(HMAC_SHA256, secret_key),
            credential_scope_date.as_bytes(),
        );

        let k_region = ring::hmac::sign(&Key::new(HMAC_SHA256, k_date.as_ref()), region.as_bytes());

        let k_service = ring::hmac::sign(
            &Key::new(HMAC_SHA256, k_region.as_ref()),
            service_name.as_bytes(),
        );

        let k_signing = ring::hmac::sign(
            &Key::new(HMAC_SHA256, k_service.as_ref()),
            "request".as_bytes(),
        );

        let signature = ring::hmac::sign(
            &Key::new(HMAC_SHA256, k_signing.as_ref()),
            string_to_sign.as_bytes(),
        );
        hex::encode(signature)
    };
    Ok((
        x_date,
        host,
        credential_scope,
        signed_headers,
        signature_string,
        canonical_request,
        string_to_sign,
    ))
}

#[cfg(test)]
mod tests {
    use crate::service_provider::volcengine::request_sign::{
        AuthHeader, create_auth_header, signature,
    };
    use std::str::FromStr;

    const AK: &str = "ak";
    const SK: &str = "sk";

    const URL: &str = "https://billing.volcengineapi.com?Action=ListBill&Version=2022-01-01";
    const BODY: &str = r#"{"Limit":10,"BillPeriod":"2023-08"}"#;
    const DATETIME: &str = "20250329T180937Z";
    #[test]
    fn test_signature() -> crate::Result<()> {
        let (
            _,
            _,
            _,
            _,
            signature_string,
            canonical_request,
            string_to_sign,
        ) = signature(
            reqwest::Method::POST,
            &url::Url::from_str(URL)?,
            Some(BODY.as_bytes()),
            chrono::NaiveDateTime::parse_from_str(DATETIME, "%Y%m%dT%H%M%SZ")?.and_utc(),
            "cn-beijing",
            "billing",
            SK.as_bytes(),
        )?;
        println!("{}", canonical_request);
        println!("{}", string_to_sign);
        println!("{}", signature_string);
        assert_eq!(
            signature_string.as_str(),
            "5e8480ceea12d0000a23c054151c50dd02c1a7dec835004057d19f13d53a7658"
        );
        Ok(())
    }

    #[test]
    fn test_create_auth_header() -> crate::Result<()> {
        let AuthHeader { auth, .. } = create_auth_header(
            reqwest::Method::POST,
            &url::Url::from_str(URL)?,
            Some(BODY.as_bytes()),
            chrono::NaiveDateTime::parse_from_str(DATETIME, "%Y%m%dT%H%M%SZ")?.and_utc(),
            "cn-beijing",
            "billing",
            AK,
            SK,
        )?;
        println!("{auth}");
        assert_eq!(
            auth,
            "HMAC-SHA256 Credential=AKLTYWViMTVmZGYzM2E0NDI5Mzk2MDZjNjFmMjc2MjRjMzg/20250329/cn-beijing/billing/request, SignedHeaders=host;x-date, Signature=5e8480ceea12d0000a23c054151c50dd02c1a7dec835004057d19f13d53a7658"
        );
        Ok(())
    }
}
