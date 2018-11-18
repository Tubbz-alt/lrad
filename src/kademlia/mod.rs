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
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tarpc::{
    client, context,
    server::{self, Handler},
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
    fn try_new(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        Ok(ContactInfo {
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            id: Identifier::try_new(id_size)?,
            round_trip_time: Duration::from_millis(0),
        })
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
    pub fn try_new(
        id_size: &IdentifierSize,
        k: usize,
        alpha: usize,
        who_am_i: ContactInfo,
    ) -> Result<Self, ErrorStack> {
        assert_eq!(*id_size, who_am_i.id.size);
        Ok(Self {
            id_size: id_size.clone(),
            k,
            alpha,
            who_am_i,
            map: HashMap::with_capacity(id_size.into()),
        })
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
struct NodeService {
    node: Arc<RwLock<Node>>,
}

impl NodeService {
    fn new(node: Arc<RwLock<Node>>) -> NodeService {
        NodeService { node }
    }

    fn try_spawn(&mut self) -> io::Result<()> {
        let address = self.node.read().unwrap().who_am_i.address;
        let transport = tarpc_bincode_transport::listen(&address)?;
        tokio_executor::spawn(
            server::Server::default()
                .incoming(transport)
                .take(1)
                .respond_with(service::serve(self.clone()))
                .unit_error()
                .boxed()
                .compat(),
        );
        Ok(())
    }

    fn bootstrap(&self) {}
}

impl self::service::Service for NodeService {
    type PingFut = Ready<Identifier>;
    type StoreFut = Self::PingFut;
    type FindNodeFut = Ready<(Identifier, Vec<ContactInfo>)>;
    type FindValueFut = Ready<(Identifier, WhoHasIt)>;
    fn ping(self, _: context::Context, magic_cookie: Identifier) -> Self::PingFut {
        future::ready(magic_cookie)
    }
    fn store(self, _: context::Context, magic_cookie: Identifier) -> Self::StoreFut {
        // TODO: add storage
        future::ready(magic_cookie)
    }
    fn find_node(
        self,
        _: context::Context,
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
