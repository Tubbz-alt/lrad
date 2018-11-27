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

pub struct DnsTxtRecordResponse {
    pub txt_data: Vec<String>,
    pub valid_until: std::time::Instant,
}

impl DnsTxtRecordResponse {
    pub fn lookup_txt_record(name: &str) -> Result<Option<Self>, ResolveError> {
        let resolver = Resolver::new(ResolverConfig::cloudflare_tls(), ResolverOpts::default())?;
        let lookup = resolver.lookup(name, RecordType::TXT)?;
        match lookup.iter().nth(0) {
            Some(rdata) => match rdata {
                RData::TXT(txt) => {
                    // We should expect that there be only one single-line or multiline string,
                    // otherwise this is open for interpretation because the string order is
                    // randomized.
                    let mut txt_data = Vec::with_capacity(txt.txt_data().len());
                    for line in txt.txt_data() {
                        let unicode_line = std::str::from_utf8(line);
                        if unicode_line.is_ok() {
                            txt_data.push(String::from(unicode_line.unwrap()));
                        }
                    }
                    Ok(Some(Self {
                        txt_data,
                        valid_until: lookup.valid_until()
                    }))
                }
                _ => Ok(None), // TODO: Could a TXT lookup return a non txt?
            },
            None => Ok(None),
        }
    }
}
