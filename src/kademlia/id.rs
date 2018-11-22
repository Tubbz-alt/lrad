use super::*;

pub trait Identifiable {
    fn id(&self) -> &Identifier;
    fn id_size(&self) -> IdentifierSize;
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum IdentifierSize {
    _512,
    _384,
    _256,
    _224,
}

impl IdentifierSize {
    fn values<'a>() -> impl Iterator<Item = &'a Self> {
        const VALUES: [IdentifierSize; 4] = [
            IdentifierSize::_512,
            IdentifierSize::_384,
            IdentifierSize::_256,
            IdentifierSize::_224,
        ];
        VALUES.iter()
    }

    pub fn as_range(self) -> std::ops::RangeInclusive<usize> {
        (1..=self.into())
    }

    pub fn hash(self, data: &[u8]) -> Identifier {
        // TODO: Use SHAKE [once supported](https://github.com/sfackler/rust-openssl/issues/1017)
        // This actually might not be possible b/c [OpenSSL doesn't have support for shake digest yet](https://www.openssl.org/docs/manmaster/man3/EVP_DigestSignInit.html)
        Identifier {
            size: self,
            bits: BitVec::from_bytes(
                match self {
                    IdentifierSize::_512 => sha::sha512(data).to_vec(),
                    IdentifierSize::_384 => sha::sha384(data).to_vec(),
                    IdentifierSize::_256 => sha::sha256(data).to_vec(),
                    IdentifierSize::_224 => sha::sha224(data).to_vec(),
                }
                .as_slice(),
            ),
        }
    }
}

impl Default for IdentifierSize {
    fn default() -> Self {
        IdentifierSize::_256
    }
}

impl Into<usize> for IdentifierSize {
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
    pub fn magic_cookie(id_size: IdentifierSize) -> Result<Self, ErrorStack> {
        let mut id_bytes = Vec::with_capacity(id_size.into());
        rand::rand_bytes(&mut id_bytes)?;
        Ok(Identifier {
            size: id_size,
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
        let size: usize = self.size.into();
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

    fn id_size(&self) -> IdentifierSize {
        self.size
    }
}

// An elliptic curve public/private key pair that represents the identity of a node.
#[derive(Eq, PartialEq, Hash, Serialize, Deserialize, Clone)]
pub struct NodeIdentity {
    public_key: Vec<u8>,
    private_key: Option<Vec<u8>>,
    id_size: IdentifierSize,
}

impl NodeIdentity {
    fn try_new(id_size: IdentifierSize) -> Result<Self, ErrorStack> {
        Self::try_from_private_key(id_size, Self::generate_ec(id_size)?)
    }

    fn try_from_private_key(
        id_size: IdentifierSize,
        key: ec::EcKey<pkey::Private>,
    ) -> Result<Self, ErrorStack> {
        let mut bn_ctx = openssl::bn::BigNumContext::new()?;
        let ec_group = key.group();
        Ok(Self {
            public_key: key.public_key().to_bytes(
                &ec_group,
                ec::PointConversionForm::COMPRESSED,
                &mut bn_ctx,
            )?,
            private_key: Some(key.private_key().to_vec()),
            id_size: id_size,
        })
    }

    fn try_from_public_key(
        id_size: IdentifierSize,
        key: ec::EcKey<pkey::Public>,
    ) -> Result<Self, ErrorStack> {
        let mut bn_ctx = openssl::bn::BigNumContext::new()?;
        let ec_group = key.group();
        Ok(Self {
            public_key: key.public_key().to_bytes(
                &ec_group,
                ec::PointConversionForm::COMPRESSED,
                &mut bn_ctx,
            )?,
            private_key: None,
            id_size: id_size,
        })
    }

    fn strip_private(&self) -> Self {
        NodeIdentity {
            public_key: self.public_key.clone(),
            private_key: None,
            id_size: self.id_size,
        }
    }

    fn generate_ec(size: IdentifierSize) -> Result<ec::EcKey<pkey::Private>, ErrorStack> {
        let ec_group = Self::ec_group(size)?;
        ec::EcKey::generate(ec_group.as_ref())
    }

    fn ec_group(id_size: IdentifierSize) -> Result<ec::EcGroup, ErrorStack> {
        ec::EcGroup::from_curve_name(Self::close_ec(id_size))
    }

    fn close_ec(id_size: IdentifierSize) -> Nid {
        match id_size {
            IdentifierSize::_512 => Nid::SECP521R1,
            IdentifierSize::_384 => Nid::SECP384R1,
            IdentifierSize::_256 => Nid::SECP256K1,
            IdentifierSize::_224 => Nid::SECP224K1,
        }
    }
}

impl Into<Identifier> for &NodeIdentity {
    fn into(self) -> Identifier {
        self.id_size.hash(self.public_key.as_slice())
    }
}

impl std::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "({:?}, REDACTED)", self.public_key)
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
    pub fn try_new(id_size: IdentifierSize) -> Result<Self, ErrorStack> {
        let node_identity = NodeIdentity::try_new(id_size)?;
        Ok(Self {
            address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080)),
            id: (&node_identity).into(),
            node_identity,
            round_trip_time: Duration::from_millis(0),
        })
    }

    pub fn new(address: SocketAddr, node_identity: NodeIdentity) -> Self {
        Self {
            address,
            id: (&node_identity).into(),
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

    fn id_size(&self) -> IdentifierSize {
        self.id.id_size()
    }
}

#[cfg(test)]
pub mod test {
    use super::{BitVec, Identifier, IdentifierSize};

    pub fn zero_id(size: IdentifierSize) -> Identifier {
        bits_id(size, BitVec::from_elem(size.into(), false))
    }

    pub fn one_id(size: IdentifierSize) -> Identifier {
        bits_id(size, BitVec::from_elem(size.into(), true))
    }

    pub fn bits_id(size: IdentifierSize, bits: BitVec) -> Identifier {
        Identifier { size: size, bits }
    }

    mod identifier {
        use super::*;

        #[test]
        fn identifier_distance_max_equals_size() {
            IdentifierSize::values()
                .for_each(|size| assert_eq!(zero_id(*size) ^ one_id(*size), (*size).into()));
        }

        #[test]
        fn identifier_distance_to_same_is_zero() {
            IdentifierSize::values()
                .for_each(|size| assert_eq!(zero_id(*size) ^ zero_id(*size), 0));
        }

        #[test]
        fn identifier_distance_from_zero_to_single_bit_on_is_single_bit_on() {
            IdentifierSize::values().for_each(|size| {
                let zero = zero_id(*size);
                let max_distance: usize = (*size).into();
                size.as_range().for_each(|x| {
                    let single_bit_id = bits_id(
                        *size,
                        BitVec::from_fn(max_distance, |index| index == max_distance - x),
                    );
                    assert_eq!(&zero ^ &single_bit_id, x);
                });
            });
        }

        #[test]
        #[should_panic]
        fn identifier_xor_panics_when_size_different() {
            assert_ne!(
                zero_id(IdentifierSize::default()) ^ zero_id(IdentifierSize::_384),
                0
            )
        }
    }
}
