use bit_vec::BitVec;
use futures::{
    future::{self, Ready},
    prelude::*,
};
use openssl::{ec, error::ErrorStack, nid::Nid, pkey, rand, sha};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tarpc::{
    context,
    server::{self, Handler},
};
use tokio::runtime;
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
    who_am_i: ContactInfo,
    table: Table<ContactInfo>,
}

impl Node {
    pub fn new(k: usize, who_am_i: ContactInfo) -> Self {
        let id = who_am_i.id().clone();
        Self {
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
}

impl Identifiable for Node {
    fn id(&self) -> &Identifier {
        self.table.id()
    }

    fn id_size(&self) -> IdentifierSize {
        self.table.id_size()
    }
}

#[derive(Clone)]
pub struct NodeService {
    node: Arc<RwLock<Node>>,
}

pub struct NodeClient {
    node: Arc<RwLock<Node>>,
    tarpc_clients: HashMap<SocketAddr, service::Client>,
}

impl NodeClient {
    fn try_new(alpha: usize, node: Arc<RwLock<Node>>) -> tokio::io::Result<Self> {
        Ok(Self {
            node,
            tarpc_clients: HashMap::new(),
            // runtime: runtime::Builder::new().core_threads(alpha).build()?, TODO: actually use alpha to concurrently ping
        })
    }
}

impl NodeClient {
    fn block_on<F, T>(future03: F) -> io::Result<T>
    where
        F: futures::Future<Output = io::Result<T>> + Send,
        T: Send,
    {
        let mut io_loop = runtime::current_thread::Runtime::new()?;
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
        let identity = self.node.read().unwrap().who_am_i.node_identity();
        let magic_cookie = Identifier::magic_cookie(self.node.read().unwrap().id_size())?;
        let client = self.get_or_connect(socket_addr)?;
        let ping_fut = client.ping(context::current(), magic_cookie.clone(), identity.clone());
        Self::block_on(ping_fut).and_then(|(responder_magic_cookie, responder_identity)| {
            if magic_cookie == responder_magic_cookie {
                Ok(Some(responder_identity))
            } else {
                Ok(None)
            }
        })
    }

    fn find_node(&mut self, id_to_find: &Identifier) -> io::Result<Option<ContactInfo>> {
        let k = self.node.read().unwrap().table.k();
        let id_size = self.node.read().unwrap().id_size();
        let mut table: Table<ContactInfo> = Table::new(id_to_find.clone(), k);
        self.node
            .read()
            .unwrap()
            .table
            .k_closest_to(id_to_find)
            .map(Clone::clone)
            .for_each(|contact| table.insert(contact));
        let mut queried: HashSet<SocketAddr> = HashSet::new();
        loop {
            let k_closest: Vec<ContactInfo> = table
                .k_closest()
                .filter(|contact| !queried.contains(&contact.address))
                .map(Clone::clone)
                .collect();
            if k_closest.len() == 0 {
                return Ok(table
                    .k_closest()
                    .find(|x| x.id() == id_to_find)
                    .map(Clone::clone));
            }
            for k_contact in k_closest {
                queried.insert(k_contact.address);
                let magic_cookie = Identifier::magic_cookie(id_size)?;
                let client = self.get_or_connect(&k_contact.address)?;
                let find_node_fut =
                    client.find_node(context::current(), magic_cookie.clone(), id_to_find.clone());
                let new_contacts = Self::block_on(find_node_fut).and_then(
                    |(responder_magic_cookie, responder_contacts)| {
                        if magic_cookie == responder_magic_cookie {
                            Ok(Some(responder_contacts))
                        } else {
                            Ok(None)
                        }
                    },
                )?;
                match new_contacts {
                    Some(new_contacts) => {
                        let mut node = self.node.write().unwrap();
                        new_contacts.iter().for_each(|new_contact| {
                            table.insert(new_contact.clone());
                            node.table.insert(new_contact.clone());
                        });
                    }
                    None => {}
                };
            }
        }
    }

    fn find_value(&mut self, value_to_find: &Identifier) -> io::Result<Option<Vec<u8>>> {
        let k = self.node.read().unwrap().table.k();
        let id_size = self.node.read().unwrap().id_size();
        let mut table: Table<ContactInfo> = Table::new(value_to_find.clone(), k);
        self.node
            .read()
            .unwrap()
            .table
            .k_closest_to(value_to_find)
            .map(Clone::clone)
            .for_each(|contact| table.insert(contact));
        let mut queried: HashSet<SocketAddr> = HashSet::new();
        loop {
            let k_closest: Vec<ContactInfo> = table
                .k_closest()
                .filter(|contact| !queried.contains(&contact.address))
                .map(Clone::clone)
                .collect();
            if k_closest.len() == 0 {
                return Ok(None);
            }
            for k_contact in k_closest {
                queried.insert(k_contact.address);
                let magic_cookie = Identifier::magic_cookie(id_size)?;
                let client = self.get_or_connect(&k_contact.address)?;
                let find_node_fut = client.find_value(
                    context::current(),
                    magic_cookie.clone(),
                    value_to_find.clone(),
                );
                let whohasit = Self::block_on(find_node_fut).and_then(
                    |(responder_magic_cookie, responder_contacts)| {
                        if magic_cookie == responder_magic_cookie {
                            Ok(Some(responder_contacts))
                        } else {
                            Ok(None)
                        }
                    },
                )?;
                match whohasit {
                    Some(whohasit) => match whohasit {
                        WhoHasIt::Me(data) => {
                            return Ok(Some(data));
                        }
                        WhoHasIt::SomeoneElse(other_contacts) => {
                            let mut node = self.node.write().unwrap();
                            other_contacts.iter().for_each(|other_contact| {
                                table.insert(other_contact.clone());
                                node.table.insert(other_contact.clone());
                            });
                        }
                    },
                    None => {}
                };
            }
        }
    }

    fn store(&mut self, data: &[u8]) -> io::Result<()> {
        let k = self.node.read().unwrap().table.k();
        let id_size = self.node.read().unwrap().id_size();
        let data_id = id_size.hash(data);

        let k_closest: Vec<ContactInfo> = self
            .node
            .read()
            .unwrap()
            .table
            .k_closest_to(&data_id)
            .map(Clone::clone)
            .collect();

        for k_contact in k_closest {
            let magic_cookie = Identifier::magic_cookie(id_size)?;
            let client = self.get_or_connect(&k_contact.address)?;
            let store_fut = client.store(
                context::current(),
                magic_cookie.clone(),
                data_id.clone(),
                data.to_vec(),
            );
            Self::block_on(store_fut).and_then(|responder_magic_cookie| {
                if magic_cookie == responder_magic_cookie {
                    Ok(())
                } else {
                    Ok(())
                }
            })?;
        }
        Ok(())
    }
}
pub enum BootstrapMethod<'a> {
    SrvRecord(&'a str),
    SocketAddr(Vec<SocketAddr>),
    Mdns(&'a str), // TODO: Add support via mdns crate
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
            _ => panic!("Unimplemented Bootstrap method!"),
        }
    }

    fn bootstrap(
        &self,
        client: &mut NodeClient,
        bootstrap_method: &BootstrapMethod,
    ) -> Result<(), ResolveError> {
        let known_contacts = self.find_contacts(bootstrap_method)?;
        let known_contacts =
            known_contacts
                .iter()
                .filter_map(|socket_addr| match client.ping(&socket_addr) {
                    Ok(Some(identity)) => Some(ContactInfo::new(*socket_addr, identity)),
                    _ => None,
                });
        let mut node = self.node.write().unwrap();
        known_contacts.for_each(|contact| node.insert(contact));

        Ok(())
    }
}

impl self::service::Service for NodeService {
    type PingFut = Ready<(Identifier, NodeIdentity)>;
    type StoreFut = Ready<Identifier>;
    type FindNodeFut = Ready<(Identifier, Vec<ContactInfo>)>;
    type FindValueFut = Ready<(Identifier, WhoHasIt)>;

    fn ping(
        self,
        _: context::Context,
        magic_cookie: Identifier,
        client_id: NodeIdentity,
    ) -> Self::PingFut {
        future::ready((
            magic_cookie,
            self.node.read().unwrap().who_am_i.node_identity(),
        ))
    }

    fn store(
        self,
        _: context::Context,
        magic_cookie: Identifier,
        data_id: Identifier,
        data: Vec<u8>,
    ) -> Self::StoreFut {
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
            self.node
                .read()
                .unwrap()
                .table
                .k_closest_to(&id_to_find)
                .map(Clone::clone)
                .collect(),
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
            WhoHasIt::SomeoneElse(
                self.node
                    .read()
                    .unwrap()
                    .table
                    .k_closest_to(&value_to_find)
                    .map(Clone::clone)
                    .collect(),
            ),
        ))
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone, Debug)]
pub enum WhoHasIt {
    Me(Vec<u8>),
    SomeoneElse(Vec<ContactInfo>),
}
