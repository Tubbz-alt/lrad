mod cloudflare;
// use crate::error::Result;

use trust_dns_proto::rr::{RData, RecordType};
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    error::ResolveError,
    Resolver,
};

pub use self::cloudflare::*;

pub trait DnsRecordPutter {
    fn try_put_txt_record(&self, ipfs_cid: String) -> crate::error::Result<bool>;
}

pub fn lookup_txt_record(name: &str) -> Result<Option<Vec<String>>, ResolveError> {
    let resolver = Resolver::new(ResolverConfig::cloudflare_tls(), ResolverOpts::default())?;
    let srv_records = resolver.lookup(name, RecordType::TXT)?;
    match srv_records.iter().nth(0) {
        Some(rdata) => match rdata {
            RData::TXT(txt) => {
                let mut txt_records = Vec::with_capacity(txt.txt_data().len());
                for line in txt.txt_data() {
                    let unicode_line = std::str::from_utf8(line);
                    if unicode_line.is_ok() {
                        txt_records.push(String::from(unicode_line.unwrap()));
                    }
                }
                Ok(Some(txt_records))
            }
            _ => Ok(None), // TODO: Could a TXT lookup return a non txt?
        },
        None => Ok(None),
    }
}
