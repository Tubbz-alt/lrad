use bit_vec::BitVec;
use futures::{
    compat::TokioDefaultSpawner,
    future::{self, Ready},
    prelude::*,
};
use openssl::{ec, error::ErrorStack, nid::Nid, pkey, rand};
use std::collections::HashMap;
use std::io;
use std::iter::FromIterator;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tarpc::{
    client, context,
    server::{self, Handler},
};
use tokio::runtime::current_thread::Runtime;
use trust_dns_proto::rr::{domain::TryParseIp, record_data::RData};
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    error::ResolveError,
    Resolver,
};

mod service;

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub enum IdentifierSize {
    _512,
    _384,
    _256,
    _224,
}

impl IdentifierSize {
    fn generate_ecdsa(self: &IdentifierSize) -> Result<ec::EcKey<pkey::Private>, ErrorStack> {
        let nid = match self {
            IdentifierSize::_512 => Nid::ECDSA_WITH_SHA512,
            IdentifierSize::_384 => Nid::ECDSA_WITH_SHA384,
            IdentifierSize::_256 => Nid::ECDSA_WITH_SHA256,
            IdentifierSize::_224 => Nid::ECDSA_WITH_SHA224,
        };
        let ec_group = ec::EcGroup::from_curve_name(nid)?;
        ec::EcKey::generate(&ec_group)
    }
}

impl Default for IdentifierSize {
    fn default() -> Self {
        IdentifierSize::_256
    }
}

impl Into<usize> for &IdentifierSize {
    fn into(self) -> usize {
        match self {
            IdentifierSize::_512 => 512,
            IdentifierSize::_384 => 384,
            IdentifierSize::_256 => 256,
            IdentifierSize::_224 => 224,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub struct Identifier {
    size: IdentifierSize,
    bits: BitVec,
}

impl Identifier {
    fn distance(&self, other: &Identifier) -> usize {
        let mut id = self.bits.clone();
        if !id.union(&other.bits) {
            0
        } else {
            id.len() - id.iter().take_while(|bit| *bit).count()
        }
    }

    fn try_new(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        let mut id_buf: Vec<u8> = Vec::with_capacity(id_size.into());
        rand::rand_bytes(&mut id_buf)?;
        Ok(Identifier {
            size: id_size.clone(),
            bits: BitVec::from_bytes(&id_buf),
        })
    }
}

#[derive(Eq, PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct ContactInfo {
    address: SocketAddr,
    id: Identifier,
    round_trip_time: Duration,
}

impl ContactInfo {
    pub fn try_new(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        Ok(Self {
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            id: Identifier::try_new(id_size)?,
            round_trip_time: Duration::from_millis(0),
        })
    }

    pub fn new(address: SocketAddr, id: Identifier) -> Self {
        Self {
            address,
            id,
            round_trip_time: Duration::from_millis(0),
        }
    }

    fn ping(&self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Bucket {
    k: usize,
    vec: Vec<ContactInfo>,
}

impl Bucket {
    fn new(k: usize) -> Self {
        Bucket {
            k,
            vec: Vec::with_capacity(k),
        }
    }

    fn update<F>(&mut self, sender: ContactInfo, ping: F)
    where
        F: FnOnce(&ContactInfo) -> bool,
    {
        self.vec.retain(|contact_info| contact_info.id != sender.id);

        if self.len() == self.k {
            match &self.vec[0].ping() {
                Ok(_) => {}
                Err(_) => {
                    self.vec.remove(0);
                    self.vec.push(sender);
                }
            };
        } else {
            self.vec.push(sender);
        }
    }

    fn iter(&self) -> impl Iterator<Item = &ContactInfo> {
        self.vec.iter()
    }

    fn len(&self) -> usize {
        self.vec.len()
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Node {
    id_size: IdentifierSize,
    alpha: usize,
    k: usize,
    who_am_i: ContactInfo,
    map: HashMap<BitVec, Bucket>,
}

impl Node {
    pub fn new(id_size: &IdentifierSize, k: usize, alpha: usize, who_am_i: ContactInfo) -> Self {
        assert_eq!(*id_size, who_am_i.id.size);
        Self {
            id_size: id_size.clone(),
            k,
            alpha,
            who_am_i,
            map: HashMap::with_capacity(id_size.into()),
        }
    }

    fn prefix(&self, distance: usize) -> Option<BitVec> {
        if distance == 0 || distance > self.k {
            None
        } else {
            let prefix_bits_it = self.who_am_i.id.bits.iter().take(self.k - distance);
            let last_prefix_bit_inverted = !self.who_am_i.id.bits[self.k - distance];
            let prefix_bits_it = prefix_bits_it.chain(vec![last_prefix_bit_inverted].into_iter());

            Some(BitVec::from_iter(prefix_bits_it))
        }
    }

    fn get(&self, distance: usize) -> Option<&Bucket> {
        self.prefix(distance)
            .and_then(|prefix| self.map.get(&prefix))
    }

    fn get_mut(&mut self, distance: usize) -> Option<&mut Bucket> {
        self.prefix(distance)
            .and_then(move |prefix| self.map.get_mut(&prefix))
    }

    fn iter(&self) -> impl Iterator<Item = &Bucket> {
        (1..=(&self.id_size).into()).filter_map(move |distance| self.get(distance))
    }

    fn k_closest(&self) -> impl Iterator<Item = &ContactInfo> {
        self.iter().flat_map(|bucket| bucket.iter()).take(self.k)
    }

    fn k_closest_to(&self, id: &Identifier) -> Vec<ContactInfo> {
        let distance = self.who_am_i.id.distance(&id);
        (distance..=(&self.id_size).into())
            .filter_map(|distance| self.get(distance))
            .flat_map(|bucket| bucket.iter())
            .take(self.k)
            .map(Clone::clone)
            .collect()
    }

    fn update(&mut self, sender: ContactInfo) {
        let distance = self.who_am_i.id.distance(&sender.id);
        match self.get_mut(distance) {
            Some(bucket) => bucket.update(sender, |_| true),
            None => (),
        };
    }
}

#[derive(Clone)]
pub struct NodeService {
    id_size: IdentifierSize,
    node: Arc<RwLock<Node>>,
}

async fn ping(id_size: IdentifierSize, socket_addr: &SocketAddr) -> io::Result<Option<Identifier>> {
    let conn = tarpc_bincode_transport::connect(socket_addr);
    let conn = await!(conn)?;
    let mut client = await!(service::new_stub(client::Config::default(), conn))?;
    let magic_cookie = Identifier::try_new(&id_size)?;
    let res = await!(client.ping(context::current(), magic_cookie.clone()))?;
    match magic_cookie == res.1 {
        true => Ok(Some(res.0)),
        false => Ok(None),
    }
}

impl NodeService {
    fn new(id_size: IdentifierSize, node: Arc<RwLock<Node>>) -> NodeService {
        NodeService { id_size, node }
    }

    fn try_spawn(self) -> io::Result<()> {
        let address = self.node.read().unwrap().who_am_i.address;
        let transport = tarpc_bincode_transport::listen(&address)?;
        tokio_executor::spawn(
            server::Server::default()
                .incoming(transport)
                .take(1)
                .respond_with(service::serve(self))
                .unit_error()
                .boxed()
                .compat(),
        );
        Ok(())
    }

    fn bootstrap(&self, srv_record_name: &str) -> Result<(), ResolveError> {
        let resolver = Resolver::new(ResolverConfig::cloudflare_tls(), ResolverOpts::default())?;
        let srv_records = resolver.lookup_srv(srv_record_name)?;
        let mut io_loop = Runtime::new()?;
        let known_peers = srv_records
            .iter()
            .filter_map(move |srv_record| {
                let target = srv_record.target().try_parse_ip()?;
                let port = srv_record.port();
                match target {
                    RData::A(ip_v4_addr) => {
                        Some(SocketAddr::V4(SocketAddrV4::new(ip_v4_addr, port)))
                    }
                    RData::AAAA(ip_v6_addr) => {
                        Some(SocketAddr::V6(SocketAddrV6::new(ip_v6_addr, port, 0, 0)))
                    }
                    _ => None,
                }
            })
            .filter_map(|socket_addr| {
                match io_loop.block_on(ping(self.id_size.clone(), &socket_addr).boxed().compat()) {
                    Ok(Some(identity)) => Some(ContactInfo::new(socket_addr, identity)),
                    _ => None,
                }
            });
        let mut node = self.node.write().unwrap();
        known_peers.for_each(|contact| node.update(contact));
        Ok(())
    }
}

impl self::service::Service for NodeService {
    type PingFut = Ready<(Identifier, Identifier)>;
    type StoreFut = Ready<Identifier>;
    type FindNodeFut = Ready<(Identifier, Vec<ContactInfo>)>;
    type FindValueFut = Ready<(Identifier, WhoHasIt)>;
    fn ping(self, _: context::Context, magic_cookie: Identifier) -> Self::PingFut {
        future::ready((self.node.read().unwrap().who_am_i.id.clone(), magic_cookie))
    }
    fn store(
        self,
        _: context::Context,
        identity: Identifier,
        magic_cookie: Identifier,
    ) -> Self::StoreFut {
        // TODO: add storage
        future::ready(magic_cookie)
    }
    fn find_node(
        self,
        _: context::Context,
        identity: Identifier,
        magic_cookie: Identifier,
        id_to_find: Identifier,
    ) -> Self::FindNodeFut {
        future::ready((
            magic_cookie,
            self.node.read().unwrap().k_closest_to(&id_to_find),
        ))
    }
    fn find_value(
        self,
        _: context::Context,
        identity: Identifier,
        magic_cookie: Identifier,
        value_to_find: Identifier,
    ) -> Self::FindValueFut {
        // TODO: add storage
        future::ready((
            magic_cookie,
            WhoHasIt::SomeoneElse(self.node.read().unwrap().k_closest_to(&value_to_find)),
        ))
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone, Debug)]
pub enum WhoHasIt {
    Me(Vec<u8>),
    SomeoneElse(Vec<ContactInfo>),
}
