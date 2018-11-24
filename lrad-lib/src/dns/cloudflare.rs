use crate::dns::DnsRecordPutter;
use crate::error::{ErrorKind, Result};

use std::env;
use std::io::Read;
use std::ops::Range;

use curl::easy::{Easy, List, ReadError};

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

#[derive(Deserialize, Serialize, Default)]
pub struct CloudflareConfig {
    email_env_var: CloudflareEmailEnvVar,
    api_key_env_var: CloudflareApiKeyEnvVar,
    zone_id_env_var: CloudflareZoneIdEnvVar,
    dns_record_id_env_var: CloudflareDnsRecordIdEnvVar,
}

impl DnsRecordPutter for CloudflareConfig {
    fn try_put_txt_record(
        &mut self,
        name: String,
        ipfs_cid: String,
        ttl: Option<usize>,
    ) -> Result<bool> {
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
        if ttl.is_some() && !VALID_TTL_RANGE.contains(&ttl.unwrap()) {}
        let mut handle = Easy::new();
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/{}",
            handle.url_encode(zone_id.1.as_bytes()),
            handle.url_encode(dns_record_id.1.as_bytes())
        );
        handle.put(true)?;
        handle.url(url.as_str())?;
        let mut headers_list = List::new();
        headers_list.append(format!("X-Auth-Email: {}", cf_email_address.1).as_str())?;
        headers_list.append(format!("X-Auth-Key: {}", cf_api_key.1).as_str())?;
        headers_list.append("Content-Type: application/json")?;
        handle.http_headers(headers_list)?;
        handle.read_function(move |into| {
            let record = DnsLinkTxtRecord::new(name.clone(), ipfs_cid.clone(), ttl);
            let record_json = serde_json::to_vec(&record).map_err(|_| ReadError::Abort)?;
            record_json
                .as_slice()
                .read(into)
                .map_err(|_| ReadError::Abort)?;
            Ok(record_json.len())
        })?;
        let mut dst = Vec::new();
        {
            let mut transfer = handle.transfer();
            transfer.write_function(|data| {
                dst.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }
        // Ok(true)
        let response: DnsRecordResponse = serde_json::from_slice(dst.as_slice())?;
        Ok(response.success)
    }
}

const VALID_TTL_RANGE: Range<usize> = (120..2147483648);

#[derive(Serialize)]
struct DnsLinkTxtRecord {
    #[serde(rename = "type")]
    record_type: &'static str,
    name: String,
    content: String,
    ttl: Option<usize>, // Technically non-negative i32
}

impl DnsLinkTxtRecord {
    fn new(name: String, ipfs_cid: String, ttl: Option<usize>) -> Self {
        DnsLinkTxtRecord {
            record_type: "TXT",
            name,
            content: format!("dnslink=/ipfs/{}", ipfs_cid),
            ttl,
        }
    }
}

#[derive(Deserialize)]
struct DnsRecordResponse {
    success: bool,
}

//                 let resolver =
//                     Resolver::new(ResolverConfig::cloudflare_tls(), ResolverOpts::default())?;
//                 let srv_records = resolver.lookup_srv(srv_record_name)?;
//                 Ok(srv_records
//                     .iter()
//                     .filter_map(move |srv_record| {
//                         let target = srv_record.target().try_parse_ip()?;
//                         let port = srv_record.port();
//                         match target {
//                             RData::A(ip_v4_addr) => {
//                                 Some(SocketAddr::V4(SocketAddrV4::new(ip_v4_addr, port)))
//                             }
//                             RData::AAAA(ip_v6_addr) => {
//                                 Some(SocketAddr::V6(SocketAddrV6::new(ip_v6_addr, port, 0, 0)))
//                             }
//                             _ => None,
//                         }
//                     })
// .collect())
