use bit_vec::BitVec;
use futures::{
    future::{self, Ready},
    prelude::*,
};
use openssl::{ec, error::ErrorStack, nid::Nid, pkey, rand, sha};
use serde::Serialize;
use std::collections::{hash_map::Entry, HashMap};
use std::convert::TryFrom;
use std::io;
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

mod collections;
mod id;
mod service;

pub use self::collections::Table;
pub use self::id::{ContactInfo, Identifiable, Identifier, IdentifierSize, NodeIdentity};

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Node {
    alpha: usize,
    who_am_i: ContactInfo,
    table: Table<ContactInfo>,
}

impl Node {
    pub fn new(k: usize, alpha: usize, who_am_i: ContactInfo) -> Self {
        let id = who_am_i.id().clone();
        Self {
            alpha,
            who_am_i,
            table: Table::new(id, k),
        }
    }

    fn update<F>(&mut self, new_contact: ContactInfo, ping: F)
    where
        F: Fn(&ContactInfo) -> bool,
    {
        self.table.update(new_contact, ping);
    }

    fn insert(&mut self, new_contact: ContactInfo) {
        self.table.insert(new_contact);
    }

    fn id_size(&self) -> &IdentifierSize {
        self.table.id_size()
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
        let mut io_loop = Runtime::new()?;
        io_loop.block_on(future03.boxed().compat())
    }

    fn get_or_connect(&mut self, socket_addr: &SocketAddr) -> io::Result<&mut service::Client> {
        if !self.tarpc_clients.contains_key(socket_addr) {
            let new_client = Self::block_on(
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
        let magic_cookie = Identifier::magic_cookie(&self.node.read().unwrap().id_size())?;
        let identity = self.node.read().unwrap().who_am_i.node_identity();
        let client = self.get_or_connect(socket_addr)?;
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
        let id_size = self.node.read().unwrap().id_size().clone();
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
        client_id: NodeIdentity,
        magic_cookie: Identifier,
    ) -> Self::PingFut {
        future::ready((
            self.node.read().unwrap().who_am_i.node_identity(),
            magic_cookie,
        ))
    }

    fn store(
        self,
        _: context::Context,
        data_id: Identifier,
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
            self.node.read().unwrap().table.k_closest_to(&id_to_find).map(Clone::clone).collect(),
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
            WhoHasIt::SomeoneElse(self.node.read().unwrap().table.k_closest_to(&value_to_find).map(Clone::clone).collect()),
        ))
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone, Debug)]
pub enum WhoHasIt {
    Me(Vec<u8>),
    SomeoneElse(Vec<ContactInfo>),
}
