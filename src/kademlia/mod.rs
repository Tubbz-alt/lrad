use bit_vec::BitVec;
use futures::future::{self, Ready};
use openssl::{error::ErrorStack, rand};
use std::collections::HashMap;
use std::io;
use std::iter::FromIterator;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;
use tarpc::context;

mod service;

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub struct Identifier {
    bits: BitVec,
}

impl Identifier {
    fn distance(&self, other: &Identifier) -> usize {
        let id = self.bits.clone();
        if !id.union(&other.bits) {
            0
        } else {
            id.len() - id.iter().take_while(|bit| *bit).count()
        }
    }

    fn try_new(id_length: usize) -> Result<Self, ErrorStack> {
        let mut id_buf: Vec<u8> = Vec::with_capacity(id_length);
        rand::rand_bytes(&mut id_buf)?;
        Ok(Identifier {
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
    fn try_new(id_length: usize) -> Result<Self, ErrorStack> {
        Ok(ContactInfo {
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            id: Identifier::try_new(id_length)?,
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
    alpha: usize,
    id_length: usize,
    k: usize,
    who_am_i: ContactInfo,
    map: HashMap<BitVec, Bucket>,
}

impl Node {
    pub fn try_new(
        alpha: usize,
        id_length: usize,
        k: usize,
        who_am_i: Option<ContactInfo>,
    ) -> Result<Self, ErrorStack> {
        let who_am_i = match who_am_i {
            Some(contact_info) => {
                assert_eq!(id_length, contact_info.id.bits.len());
                contact_info
            }
            None => ContactInfo::try_new(id_length)?,
        };
        Ok(Node {
            alpha,
            id_length,
            k,
            who_am_i,
            map: HashMap::new(),
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
            .and_then(|prefix| self.map.get_mut(&prefix))
    }

    fn iter(&self) -> impl Iterator<Item = &Bucket> {
        (1..=self.id_length).filter_map(|distance| self.get(distance))
    }

    fn k_closest(&self) -> impl Iterator<Item = &ContactInfo> {
        self.iter().flat_map(|bucket| bucket.iter()).take(self.k)
    }

    fn k_closest_to(&self, id: &Identifier) -> Vec<ContactInfo> {
        let distance = self.who_am_i.id.distance(&id);
        (distance..=self.id_length)
            .filter_map(|distance| self.get(distance))
            .flat_map(|bucket| bucket.iter())
            .take(self.k)
            .map(Clone::clone)
            .collect()
    }

    fn update(&mut self, sender: ContactInfo) {
        match self.get_mut(self.who_am_i.id.distance(&sender.id)) {
            Some(bucket) => bucket.update(sender, |_| true),
            None => (),
        };
    }
}

use self::service::*;

impl self::service::Service for Node {
    type PingFut = Ready<Identifier>;
    type StoreFut = Self::PingFut;
    type FindNodeFut = Ready<(Identifier, Vec<ContactInfo>)>;
    type FindValueFut = Ready<(Identifier, WhoHasIt)>;
    fn ping(self, _: context::Context, magic_cookie: Identifier) -> Self::PingFut {
        future::ready(magic_cookie)
    }
    fn store(self, _: context::Context, magic_cookie: Identifier) -> Self::StoreFut {
        future::ready(magic_cookie)
    }
    fn find_node(
        self,
        _: context::Context,
        magic_cookie: Identifier,
        id_to_find: Identifier,
    ) -> Self::FindNodeFut {
        future::ready((magic_cookie, self.k_closest_to(&id_to_find)))
    }
    fn find_value(
        self,
        _: context::Context,
        magic_cookie: Identifier,
        value_to_find: Identifier,
    ) -> Self::FindValueFut {
        unimplemented!();
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone, Debug)]
pub enum WhoHasIt {
    Me(Vec<u8>),
    SomeoneElse(Vec<ContactInfo>),
}
