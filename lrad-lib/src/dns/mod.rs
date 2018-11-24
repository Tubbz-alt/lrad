mod cloudflare;
use crate::error::Result;

pub use self::cloudflare::*;

pub trait DnsRecordPutter {
    fn try_put_txt_record(
        &mut self,
        name: String,
        ipfs_cid: String,
        ttl: Option<usize>,
    ) -> Result<bool>;
}
