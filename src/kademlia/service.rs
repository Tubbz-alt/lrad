use super::*;
service! {
    rpc ping(magic_cookie: Identifier, client_id: NodeIdentity) -> (Identifier, NodeIdentity);
    rpc store(magic_cookie: Identifier, data_id: Identifier, data: Vec<u8>) -> Identifier;
    rpc find_node(magic_cookie: Identifier, id_to_find: Identifier) -> (Identifier, Vec<ContactInfo>);
    rpc find_value(magic_cookie: Identifier, value_to_find: Identifier) -> (Identifier, WhoHasIt);
}
