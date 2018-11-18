use super::{ContactInfo, Identifier, WhoHasIt};
service! {
    rpc ping(magic_cookie: Identifier) -> Identifier;
    rpc store(magic_cookie: Identifier) -> Identifier;
    rpc find_node(magic_cookie: Identifier, id_to_find: Identifier) -> (Identifier, Vec<ContactInfo>);
    rpc find_value(magic_cookie: Identifier, value_to_find: Identifier) -> (Identifier, WhoHasIt);
}
