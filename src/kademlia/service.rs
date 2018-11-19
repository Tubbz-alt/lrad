use super::{ContactInfo, Identifier, WhoHasIt};
service! {
    rpc ping(magic_cookie: Identifier) -> (Identifier, Identifier);
    rpc store(identity: Identifier, magic_cookie: Identifier) -> Identifier;
    rpc find_node(identity: Identifier, magic_cookie: Identifier, id_to_find: Identifier) -> (Identifier, Vec<ContactInfo>);
    rpc find_value(identity: Identifier, magic_cookie: Identifier, value_to_find: Identifier) -> (Identifier, WhoHasIt);
}
