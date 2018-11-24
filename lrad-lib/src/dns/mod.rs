mod cloudflare;
use crate::error::Result;

pub use self::cloudflare::*;

pub trait DnsRecordPutter {
    fn try_put_txt_record(&self, ipfs_cid: String) -> Result<bool>;
}
