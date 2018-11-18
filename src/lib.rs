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
extern crate trust_dns_resolver;

const BIND_PORT: usize = 16840;

mod kademlia;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
