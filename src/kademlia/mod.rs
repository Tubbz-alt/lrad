use bit_vec::BitVec;
use futures::{
    compat::TokioDefaultSpawner,
    future::{self, Ready},
    prelude::*,
};
use openssl::{ec, error::ErrorStack, nid::Nid, pkey, rand, sha};
use std::collections::{hash_map::Entry, HashMap};
use std::convert::TryFrom;
use std::io;
use std::iter::FromIterator;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tarpc::{
    context,
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

#[derive(Eq, PartialEq, Hash, Serialize, Deserialize, Clone)]
pub struct NodeIdentity {
    public_key: Vec<u8>,
    private_key: Option<Vec<u8>>,
}

impl NodeIdentity {
    fn try_new(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        Self::try_from(id_size.generate_ec()?)
    }

    fn strip_private(&self) -> Self {
        NodeIdentity {
            public_key: self.public_key.clone(),
            private_key: None,
        }
    }
}

impl std::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "({:?}, REDACTED)", self.public_key)
    }
}

impl TryFrom<ec::EcKey<pkey::Private>> for NodeIdentity {
    type Error = ErrorStack;

    fn try_from(key: ec::EcKey<pkey::Private>) -> Result<Self, Self::Error> {
        let mut bn_ctx = openssl::bn::BigNumContext::new()?;
        let ec_group = key.group();
        Ok(Self {
            public_key: key.public_key().to_bytes(
                &ec_group,
                ec::PointConversionForm::COMPRESSED,
                &mut bn_ctx,
            )?,
            private_key: Some(key.private_key().to_vec()),
        })
    }
}

impl TryFrom<ec::EcKey<pkey::Public>> for NodeIdentity {
    type Error = ErrorStack;

    fn try_from(key: ec::EcKey<pkey::Public>) -> Result<Self, Self::Error> {
        let mut bn_ctx = openssl::bn::BigNumContext::new()?;
        let ec_group = key.group();
        Ok(Self {
            public_key: key.public_key().to_bytes(
                &ec_group,
                ec::PointConversionForm::COMPRESSED,
                &mut bn_ctx,
            )?,
            private_key: None,
        })
    }
}

impl IdentifierSize {
    fn generate_ec(&self) -> Result<ec::EcKey<pkey::Private>, ErrorStack> {
        let ec_group = self.ec_group()?;
        ec::EcKey::generate(ec_group.as_ref())
    }

    fn ec_group(&self) -> Result<ec::EcGroup, ErrorStack> {
        ec::EcGroup::from_curve_name(self.close_ec())
    }

    fn hash(&self, bytes_to_hash: &[u8]) -> Vec<u8> {
        match self {
            IdentifierSize::_512 => sha::sha512(bytes_to_hash).to_vec(),
            IdentifierSize::_384 => sha::sha384(bytes_to_hash).to_vec(),
            IdentifierSize::_256 => sha::sha256(bytes_to_hash).to_vec(),
            IdentifierSize::_224 => sha::sha224(bytes_to_hash).to_vec(),
        }
    }

    fn close_ec(&self) -> Nid {
        match self {
            IdentifierSize::_512 => Nid::SECP521R1,
            IdentifierSize::_384 => Nid::SECP384R1,
            IdentifierSize::_256 => Nid::SECP256K1,
            IdentifierSize::_224 => Nid::SECP224K1,
        }
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

    fn new(identity: &NodeIdentity, id_size: &IdentifierSize) -> Self {
        Identifier {
            size: id_size.clone(),
            bits: BitVec::from_bytes(&id_size.hash(identity.public_key.as_slice())),
        }
    }

    fn magic_cookie(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        let mut id_bytes = Vec::with_capacity(id_size.into());
        rand::rand_bytes(&mut id_bytes)?;
        Ok(Identifier {
            size: id_size.clone(),
            bits: BitVec::from_bytes(&id_bytes),
        })
    }
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub struct ContactInfo {
    address: SocketAddr,
    id: Identifier,
    node_identity: NodeIdentity,
    round_trip_time: Duration,
}

impl ContactInfo {
    pub fn try_new(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        let node_identity = NodeIdentity::try_new(&id_size)?;
        Ok(Self {
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            id: Identifier::new(&node_identity, id_size),
            node_identity,
            round_trip_time: Duration::from_millis(0),
        })
    }

    pub fn new(address: SocketAddr, id_size: &IdentifierSize, node_identity: NodeIdentity) -> Self {
        Self {
            address,
            id: Identifier::new(&node_identity, id_size),
            node_identity,
            round_trip_time: Duration::from_millis(0),
        }
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Bucket<T> {
    k: usize,
    vec: Vec<T>,
}

impl<T: PartialEq + Clone> Bucket<T> {
    fn new(k: usize) -> Self {
        Bucket {
            k,
            vec: Vec::with_capacity(k),
        }
    }

    fn update<F>(&mut self, new_contact: T, ping: F)
    where
        F: Fn(&T) -> bool,
    {
        self.vec.retain(|contact_info| *contact_info != new_contact);

        if self.len() == self.k {
            match ping(&self.vec[0]) {
                true => {}
                false => {
                    self.vec.remove(0);
                    self.vec.push(new_contact);
                }
            };
        } else {
            self.vec.push(new_contact);
        }
    }

    fn insert(&mut self, new_contact: T) {
        self.vec.retain(|contact_info| *contact_info != new_contact);
        if self.len() == self.k {
            self.vec.remove(0);
        }
        self.vec.push(new_contact);
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        self.vec.iter()
    }

    fn len(&self) -> usize {
        self.vec.len()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn bucket_insert_stops_at_k_and_erases_older() {
        let mut bucket = super::Bucket::new(3);
        bucket.insert(1);
        bucket.insert(2);
        bucket.insert(3);
        bucket.insert(4);
        assert_eq!(bucket.len(), 3);
        assert_eq!(bucket.vec, vec![2, 3, 4]);
    }

    #[test]
    fn bucket_update_stops_at_k_and_keeps_older_when_pings_succeed() {
        let ping_succeeds = |_: &i32| true;
        let mut bucket = super::Bucket::new(3);
        bucket.update(1, ping_succeeds);
        bucket.update(2, ping_succeeds);
        bucket.update(3, ping_succeeds);
        bucket.update(4, ping_succeeds);
        assert_eq!(bucket.len(), 3);
        assert_eq!(bucket.vec, vec![1, 2, 3]);
    }

    #[test]
    fn bucket_update_stops_at_k_and_removes_older_when_pings_fail() {
        let ping_succeeds = |_: &i32| false;
        let mut bucket = super::Bucket::new(3);
        bucket.update(1, ping_succeeds);
        bucket.update(2, ping_succeeds);
        bucket.update(3, ping_succeeds);
        bucket.update(4, ping_succeeds);
        assert_eq!(bucket.len(), 3);
        assert_eq!(bucket.vec, vec![2, 3, 4]);
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Node {
    id_size: IdentifierSize,
    alpha: usize,
    k: usize,
    who_am_i: ContactInfo,
    map: HashMap<BitVec, Bucket<ContactInfo>>,
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

    fn get(&self, distance: usize) -> Option<&Bucket<ContactInfo>> {
        self.prefix(distance)
            .and_then(|prefix| self.map.get(&prefix))
    }

    fn get_mut(&mut self, distance: usize) -> Option<&mut Bucket<ContactInfo>> {
        self.prefix(distance)
            .and_then(move |prefix| self.map.get_mut(&prefix))
    }

    fn entry(&mut self, distance: usize) -> Option<Entry<BitVec, Bucket<ContactInfo>>> {
        self.prefix(distance)
            .and_then(move |prefix| Some(self.map.entry(prefix)))
    }

    fn iter(&self) -> impl Iterator<Item = &Bucket<ContactInfo>> {
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

    fn update<F>(&mut self, new_contact: ContactInfo, ping: F)
    where
        F: Fn(&ContactInfo) -> bool,
    {
        let distance = self.who_am_i.id.distance(&new_contact.id);
        let k = self.k;
        self.entry(distance)
            .expect("Distance should be in range")
            .or_insert(Bucket::new(k))
            .update(new_contact, ping);
    }

    fn insert(&mut self, new_contact: ContactInfo) {
        let distance = self.who_am_i.id.distance(&new_contact.id);
        let k = self.k;
        let bucket = self
            .entry(distance)
            .expect("Distance should be in range")
            .or_insert(Bucket::new(k));
        bucket.insert(new_contact);
    }
}

#[derive(Clone)]
pub struct NodeService {
    node: Arc<RwLock<Node>>,
}

#[derive(Clone)]
pub struct NodeClient {
    node: Arc<RwLock<Node>>,
    tarpc_clients: HashMap<SocketAddr, service::Client>,
}

impl From<Arc<RwLock<Node>>> for NodeClient {
    fn from(node: Arc<RwLock<Node>>) -> Self {
        Self {
            node,
            tarpc_clients: HashMap::new(),
        }
    }
}

impl NodeClient {
    fn block_on<F, T>(future03: F) -> io::Result<T>
    where
        F: futures::Future<Output = io::Result<T>>,
    {
        use futures::task::SpawnExt;
        let mut io_loop = Runtime::new()?;
        io_loop.block_on(future03.boxed().compat())
    }

    fn get_or_connect(&mut self, socket_addr: &SocketAddr) -> io::Result<&mut service::Client> {
        if !self.tarpc_clients.contains_key(socket_addr) {
            let mut new_client = Self::block_on(
                async {
                    use tarpc::client;
                    let conn = tarpc_bincode_transport::connect(socket_addr);
                    let conn = await!(conn)?;
                    await!(service::new_stub(client::Config::default(), conn))
                },
            )?;

            self.tarpc_clients.insert(socket_addr.clone(), new_client);
        }
        Ok(self.tarpc_clients.get_mut(socket_addr).unwrap())
    }

    fn ping(&mut self, socket_addr: &SocketAddr) -> io::Result<Option<NodeIdentity>> {
        let magic_cookie = Identifier::magic_cookie(&self.node.read().unwrap().id_size)?;
        let identity = self
            .node
            .read()
            .unwrap()
            .who_am_i
            .node_identity
            .strip_private();
        let mut client = self.get_or_connect(socket_addr)?;
        let res = Self::block_on(client.ping(context::current(), identity, magic_cookie.clone()))?;
        match magic_cookie == res.1 {
            true => Ok(Some(res.0)),
            false => Ok(None),
        }
    }
}
pub enum BootstrapMethod<'a> {
    SrvRecord(&'a str),
    SocketAddr(Vec<SocketAddr>),
}

impl From<Arc<RwLock<Node>>> for NodeService {
    fn from(node: Arc<RwLock<Node>>) -> Self {
        Self { node }
    }
}

impl NodeService {
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

    fn find_contacts(
        &self,
        bootstrap_method: &BootstrapMethod,
    ) -> Result<Vec<SocketAddr>, ResolveError> {
        match bootstrap_method {
            BootstrapMethod::SocketAddr(socket_addrs) => Ok(socket_addrs.clone()),
            BootstrapMethod::SrvRecord(srv_record_name) => {
                let resolver =
                    Resolver::new(ResolverConfig::cloudflare_tls(), ResolverOpts::default())?;
                let srv_records = resolver.lookup_srv(srv_record_name)?;
                Ok(srv_records
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
                    .collect())
            }
        }
    }

    fn bootstrap(
        &self,
        client: &mut NodeClient,
        bootstrap_method: &BootstrapMethod,
    ) -> Result<(), ResolveError> {
        let known_contacts = self.find_contacts(bootstrap_method)?;
        let id_size = self.node.read().unwrap().id_size.clone();
        let known_contacts =
            known_contacts
                .iter()
                .filter_map(|socket_addr| match client.ping(&socket_addr) {
                    Ok(Some(identity)) => {
                        Some(ContactInfo::new(socket_addr.clone(), &id_size, identity))
                    }
                    _ => None,
                });
        let mut node = self.node.write().unwrap();
        known_contacts.for_each(|contact| node.insert(contact));

        Ok(())
    }
}

impl self::service::Service for NodeService {
    type PingFut = Ready<(NodeIdentity, Identifier)>;
    type StoreFut = Ready<Identifier>;
    type FindNodeFut = Ready<(Identifier, Vec<ContactInfo>)>;
    type FindValueFut = Ready<(Identifier, WhoHasIt)>;

    fn ping(
        self,
        _: context::Context,
        client_identity: NodeIdentity,
        magic_cookie: Identifier,
    ) -> Self::PingFut {
        future::ready((
            self.node
                .read()
                .unwrap()
                .who_am_i
                .node_identity
                .strip_private(),
            magic_cookie,
        ))
    }

    fn store(
        self,
        _: context::Context,
        identity: Identifier,
        data: Vec<u8>,
        magic_cookie: Identifier,
    ) -> Self::StoreFut {
        // TODO: add storage
        future::ready(magic_cookie)
    }

    fn find_node(
        self,
        _: context::Context,
        id_to_find: Identifier,
        magic_cookie: Identifier,
    ) -> Self::FindNodeFut {
        future::ready((
            magic_cookie,
            self.node.read().unwrap().k_closest_to(&id_to_find),
        ))
    }

    fn find_value(
        self,
        _: context::Context,
        value_to_find: Identifier,
        magic_cookie: Identifier,
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
