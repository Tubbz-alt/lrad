use super::*;

pub trait Identifiable {
    fn id(&self) -> &Identifier;
    fn id_size(&self) -> &IdentifierSize;
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub enum IdentifierSize {
    _512,
    _384,
    _256,
    _224,
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

    fn values<'a>() -> impl Iterator<Item = &'a Self> {
        const VALUES: [IdentifierSize; 4] = [
            IdentifierSize::_512,
            IdentifierSize::_384,
            IdentifierSize::_256,
            IdentifierSize::_224,
        ];
        VALUES.iter()
    }

    pub fn as_range(&self) -> std::ops::RangeInclusive<usize> {
        (1..=self.into())
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

impl Into<usize> for IdentifierSize {
    fn into(self) -> usize {
        (&self).into()
    }
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub struct Identifier {
    size: IdentifierSize,
    bits: BitVec,
}

impl Identifier {
    pub fn new(identity: &NodeIdentity, id_size: &IdentifierSize) -> Self {
        Identifier {
            size: id_size.clone(),
            bits: BitVec::from_bytes(&id_size.hash(identity.public_key.as_slice())),
        }
    }

    pub fn magic_cookie(id_size: &IdentifierSize) -> Result<Self, ErrorStack> {
        let mut id_bytes = Vec::with_capacity(id_size.into());
        rand::rand_bytes(&mut id_bytes)?;
        Ok(Identifier {
            size: id_size.clone(),
            bits: BitVec::from_bytes(&id_bytes),
        })
    }
}

impl std::ops::BitXor for &Identifier {
    type Output = usize;
    fn bitxor(self, rhs: Self) -> Self::Output {
        assert_eq!(self.size, rhs.size);
        let prefix_bits = self
            .bits
            .iter()
            .zip(rhs.bits.iter())
            .take_while(|(a, b)| a == b)
            .count();
        println!("prefix is {}", prefix_bits);
        let size: usize = (&self.size).into();
        size - prefix_bits
    }
}

impl std::ops::BitXor for Identifier {
    type Output = usize;
    fn bitxor(self, rhs: Self) -> Self::Output {
        &self ^ &rhs
    }
}

impl Identifiable for Identifier {
    fn id(&self) -> &Self {
        self
    }

    fn id_size(&self) -> &IdentifierSize {
        &self.size
    }
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

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
pub struct ContactInfo {
    pub address: SocketAddr,
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

    pub fn node_identity(&self) -> NodeIdentity {
        self.node_identity.strip_private()
    }
}

impl Identifiable for ContactInfo {
    fn id(&self) -> &Identifier {
        &self.id
    }

    fn id_size(&self) -> &IdentifierSize {
        &self.id.id_size()
    }
}

#[cfg(test)]
pub mod test {
    use super::{BitVec, Identifier, IdentifierSize};

    pub fn zero_id(size: &IdentifierSize) -> Identifier {
        bits_id(size, BitVec::from_elem(size.into(), false))
    }

    pub fn one_id(size: &IdentifierSize) -> Identifier {
        bits_id(size, BitVec::from_elem(size.into(), true))
    }

    pub fn bits_id(size: &IdentifierSize, bits: BitVec) -> Identifier {
        Identifier {
            size: size.clone(),
            bits,
        }
    }

    mod identifier {
        use super::*;

        #[test]
        fn identifier_distance_max_equals_size() {
            IdentifierSize::values()
                .for_each(|size| assert_eq!(zero_id(size) ^ one_id(size), size.into()));
        }

        #[test]
        fn identifier_distance_to_same_is_zero() {
            IdentifierSize::values().for_each(|size| assert_eq!(zero_id(size) ^ zero_id(size), 0));
        }

        #[test]
        fn identifier_distance_from_zero_to_single_bit_on_is_single_bit_on() {
            IdentifierSize::values().for_each(|size| {
                let zero = zero_id(size);
                size.as_range().into_iter().map(|x| {
                    let single_bit_id =
                        bits_id(&size, BitVec::from_fn(size.into(), |index| index != x));
                    assert_eq!(&zero ^ &single_bit_id, x);
                });
            });
        }

        #[test]
        #[should_panic]
        fn identifier_xor_panics_when_size_different() {
            zero_id(&IdentifierSize::default()) ^ zero_id(&IdentifierSize::_384);
        }
    }
}
