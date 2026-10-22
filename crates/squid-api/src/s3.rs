//! Minimal S3-compatible object storage client (path-style, SigV4).

use anyhow::Context;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use squid_core::S3Config;

#[derive(Clone)]
pub struct S3Storage {
    endpoint: String,
    bucket: String,
    region: String,
    access_key_id: String,
    secret_access_key: String,
    client: reqwest::Client,
}

impl S3Storage {
    pub fn new(config: S3Config) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("failed to build S3 HTTP client")?;
        Ok(Self {
            endpoint: config.endpoint,
            bucket: config.bucket,
            region: config.region,
            access_key_id: config.access_key_id,
            secret_access_key: config.secret_access_key,
            client,
        })
    }

    pub async fn put(&self, key: &str, body: &[u8], content_type: &str) -> anyhow::Result<()> {
        let (url, authorization, date, payload_hash) = self
            .sign("PUT", key, body)
            .context("failed to sign S3 PUT")?;
        let response = self
            .client
            .put(url)
            .header("authorization", authorization)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", date)
            .header("content-type", content_type)
            .body(body.to_vec())
            .send()
            .await
            .context("S3 PUT request failed")?;
        if !response.status().is_success() {
            anyhow::bail!("S3 PUT returned {}", response.status());
        }
        Ok(())
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let (url, authorization, date, payload_hash) = self
            .sign("GET", key, &[])
            .context("failed to sign S3 GET")?;
        let response = self
            .client
            .get(url)
            .header("authorization", authorization)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", date)
            .send()
            .await
            .context("S3 GET request failed")?;
        if !response.status().is_success() {
            anyhow::bail!("S3 GET returned {}", response.status());
        }
        Ok(response.bytes().await?.to_vec())
    }

    fn sign(
        &self,
        method: &str,
        key: &str,
        body: &[u8],
    ) -> anyhow::Result<(String, String, String, String)> {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let host = self
            .endpoint
            .split("://")
            .nth(1)
            .and_then(|value| value.split('/').next())
            .ok_or_else(|| anyhow::anyhow!("S3 endpoint must include a host"))?;
        let canonical_uri = format!(
            "/{}/{}",
            percent_encode(&self.bucket),
            key.split('/')
                .map(percent_encode)
                .collect::<Vec<_>>()
                .join("/")
        );
        let payload_hash = hex::encode(Sha256::digest(body));
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical_request.as_bytes()))
        );
        let signing_key = signing_key(&self.secret_access_key, &date, &self.region);
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&signing_key).expect("HMAC accepts any key length");
        mac.update(string_to_sign.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key_id
        );
        let url = format!("{}/{}/{}", self.endpoint, self.bucket, key);
        Ok((url, authorization, amz_date, payload_hash))
    }
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(format!("AWS4{secret}").as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(date.as_bytes());
    let date_key = mac.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&date_key).expect("HMAC accepts any key length");
    mac.update(region.as_bytes());
    let region_key = mac.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&region_key).expect("HMAC accepts any key length");
    mac.update(b"s3");
    let service_key = mac.finalize().into_bytes();
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&service_key).expect("HMAC accepts any key length");
    mac.update(b"aws4_request");
    mac.finalize().into_bytes().to_vec()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
