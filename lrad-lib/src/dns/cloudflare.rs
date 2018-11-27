use crate::dns::DnsRecordPutter;
use crate::error::{ErrorKind, Result};

use std::env;
use std::ops::Range;
use std::sync::mpsc;

use actix_web::client;
use actix_web::HttpMessage;
use futures::prelude::*;

#[derive(Deserialize, Serialize)]
struct CloudflareApiKeyEnvVar(String);

impl Default for CloudflareApiKeyEnvVar {
    fn default() -> Self {
        Self(String::from("CF_API_KEY"))
    }
}

#[derive(Deserialize, Serialize)]
struct CloudflareEmailEnvVar(String);

impl Default for CloudflareEmailEnvVar {
    fn default() -> Self {
        Self(String::from("CF_EMAIL"))
    }
}

#[derive(Deserialize, Serialize)]
struct CloudflareDnsRecordIdEnvVar(String);

impl Default for CloudflareDnsRecordIdEnvVar {
    fn default() -> Self {
        Self(String::from("CF_DNS_RECORD_ID"))
    }
}

#[derive(Deserialize, Serialize)]
struct CloudflareZoneIdEnvVar(String);

impl Default for CloudflareZoneIdEnvVar {
    fn default() -> Self {
        Self(String::from("CF_ZONE_ID"))
    }
}

#[derive(Deserialize, Serialize, Clone, Copy)]
struct CloudflareDnsRecordTTL(u32);

impl Default for CloudflareDnsRecordTTL {
    fn default() -> Self {
        Self(1)
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct CloudflareConfig {
    email_env_var: CloudflareEmailEnvVar,
    api_key_env_var: CloudflareApiKeyEnvVar,
    zone_id_env_var: CloudflareZoneIdEnvVar,
    dns_record_id_env_var: CloudflareDnsRecordIdEnvVar,
    dns_record_name: String,
    dns_record_ttl: Option<CloudflareDnsRecordTTL>,
}

impl DnsRecordPutter for CloudflareConfig {
    fn try_put_txt_record(&self, ipfs_cid: String) -> Result<bool> {
        debug!("Reading environment variables");
        let cf_email_address = env::vars()
            .find(|x| x.0 == self.email_env_var.0)
            .ok_or_else(|| ErrorKind::EnvironmentVariableNotFound(self.email_env_var.0.clone()))?;
        let cf_api_key = env::vars()
            .find(|x| x.0 == self.api_key_env_var.0)
            .ok_or_else(|| {
                ErrorKind::EnvironmentVariableNotFound(self.api_key_env_var.0.clone())
            })?;
        let zone_id = env::vars()
            .find(|x| x.0 == self.zone_id_env_var.0)
            .ok_or_else(|| {
                ErrorKind::EnvironmentVariableNotFound(self.zone_id_env_var.0.clone())
            })?;
        let dns_record_id = env::vars()
            .find(|x| x.0 == self.dns_record_id_env_var.0)
            .ok_or_else(|| {
                ErrorKind::EnvironmentVariableNotFound(self.dns_record_id_env_var.0.clone())
            })?;
        let dns_record_name = self.dns_record_name;
        let dns_record_ttl = self.dns_record_ttl.unwrap_or_default().0;
        if dns_record_ttl != 1 && !VALID_TTL_RANGE.contains(&dns_record_ttl) {
            // TODO: Actually handle this
            panic!("Invalid TTL: {}", dns_record_ttl);
        }
        debug!("Building actix-web request");
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
            zone_id.1, dns_record_id.1
        );
        let record =
            DnsLinkTxtRecord::new(dns_record_name.1.clone(), ipfs_cid.clone(), dns_record_ttl);
        debug!("Sending CF put request...");
        let (tx, rx) = mpsc::channel();
        actix::run(|| {
            client::put(url)
                .header("X-Auth-Email", cf_email_address.1)
                .header("X-Auth-Key", cf_api_key.1)
                .content_type("application/json")
                .json(record)
                .unwrap()
                .send()
                .map_err(|_| ())
                .and_then(|res| {
                    debug!("Parsing CF put response...");
                    res.json().map_err(|_| ())
                })
                .and_then(move |json: DnsRecordResponse| {
                    debug!("Moving CF put response...");
                    tx.send(json.clone()).unwrap();
                    actix::System::current().stop();
                    Ok(())
                })
        });
        debug!("Done sending CF");
        let response = rx.recv().unwrap();
        Ok(response.success)
    }
}

const VALID_TTL_RANGE: Range<u32> = (120..2147483648);

#[derive(Serialize)]
struct DnsLinkTxtRecord {
    #[serde(rename = "type")]
    record_type: &'static str,
    name: String,
    content: String,
    ttl: u32, // Technically non-negative i32
}

impl DnsLinkTxtRecord {
    fn new(name: String, ipfs_cid: String, ttl: u32) -> Self {
        DnsLinkTxtRecord {
            record_type: "TXT",
            name,
            content: format!("dnslink=/ipfs/{}", ipfs_cid),
            ttl,
        }
    }
}

#[derive(Deserialize, Clone)]
struct DnsRecordResponse {
    success: bool,
}
