use super::*;
service! {
    rpc ping(client_id: NodeIdentity, magic_cookie: Identifier) -> (NodeIdentity, Identifier);
    rpc store(data_id: Identifier, data: Vec<u8>, magic_cookie: Identifier) -> Identifier;
    rpc find_node(id_to_find: Identifier, magic_cookie: Identifier) -> (Identifier, Vec<ContactInfo>);
    rpc find_value(value_to_find: Identifier, magic_cookie: Identifier) -> (Identifier, WhoHasIt);
}
