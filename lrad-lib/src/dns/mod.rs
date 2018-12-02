mod cloudflare;

use ::actix::prelude::*;
use futures::prelude::*;
use trust_dns_proto::rr::{RData, RecordType};
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    AsyncResolver,
};

use crate::error::Error;

pub use self::cloudflare::*;

pub trait DnsRecordPutter {
    fn try_put_txt_record(&self, ipfs_cid: String) -> crate::error::BoxFuture<bool>;
}

#[derive(Clone)]
pub struct DnsTxtRecordResponse {
    pub txt_data: Vec<String>,
    // pub valid_until: std::time::Instant,
}

impl DnsTxtRecordResponse {
    pub fn lookup_txt_record(name: &str) -> impl Future<Item = Option<Self>, Error = Error> {
        debug!("Looking up {}", name);
        let resolver = AsyncResolver::new(ResolverConfig::cloudflare(), ResolverOpts::default());
        Arbiter::spawn(resolver.1);
        let resolver = resolver.0;
        resolver
            .txt_lookup(name)
            .and_then(|lookup| match lookup.iter().nth(0) {
                Some(txt) => {
                    // We should expect that there be only one single-line or multiline string,
                    // otherwise this is open for interpretation because the string order is
                    // randomized.
                    debug!("Received response, parsing");
                    let mut txt_data = Vec::with_capacity(txt.txt_data().len());
                    for line in txt.txt_data() {
                        let unicode_line = std::str::from_utf8(line);
                        if unicode_line.is_ok() {
                            txt_data.push(String::from(unicode_line.unwrap()));
                        }
                    }
                    debug!("Returning response");
                    Ok(Some(Self { txt_data }))
                }
                None => Ok(None),
            })
            .map_err(|err| err.into())
    }

    pub fn as_hash(&self) -> Option<&str> {
        self.txt_data
            .first()
            .and_then(|x| x.get("dnslink=/ipfs/".len()..))
    }
}

// TODO: convert to actix actors
// impl Message for DnsTxtRecordResponse {
//     type Result = Result<Option<Self>>;
// }

impl PartialEq for DnsTxtRecordResponse {
    fn eq(&self, other: &Self) -> bool {
        self.txt_data == other.txt_data
    }
}
