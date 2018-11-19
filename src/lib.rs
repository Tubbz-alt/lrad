#![feature(
    futures_api,
    pin,
    arbitrary_self_types,
    await_macro,
    async_await,
    proc_macro_hygiene
)]
#![feature(existential_type)]
extern crate openssl;
#[macro_use]
extern crate tarpc;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate bit_vec;
extern crate futures;
extern crate tarpc_bincode_transport;
extern crate tokio;
extern crate tokio_executor;
extern crate trust_dns_proto;
extern crate trust_dns_resolver;

const BIND_PORT: usize = 16840;
const SRV_RECORD: &str = "_lrad._tcp.spuri.io";

mod kademlia;

#[cfg(test)]
mod tests {
    use super::kademlia;
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
        let id_size = kademlia::IdentifierSize::default();
        let node = kademlia::Node::new(
            &id_size,
            20,
            3,
            kademlia::ContactInfo::try_new(&id_size)
                .expect("Random contact successfully generated"),
        );
    }
}
