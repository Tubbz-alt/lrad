use super::id::*;
use super::*;

use std::collections::BTreeMap;

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Bucket<T: PartialEq> {
    k: usize,
    vec: Vec<T>, // TODO: Use a stack-allocated LRU cache
}

impl<T: PartialEq> Bucket<T> {
    fn new(k: usize) -> Self {
        Bucket {
            k,
            vec: Vec::with_capacity(k),
        }
    }

    fn update<F>(&mut self, value: T, ping: F)
    where
        F: Fn(&T) -> bool,
    {
        self.vec.retain(|element| *element != value);

        if self.len() == self.k {
            match ping(&self.vec[0]) {
                true => {} // TODO: store the new node in a cache, an optimization for Kademlia
                false => {
                    self.vec.remove(0);
                    self.vec.push(value);
                }
            };
        } else {
            self.vec.push(value);
        }
    }

    fn insert(&mut self, value: T) {
        self.vec.retain(|element| *element != value);
        if self.len() == self.k {
            self.vec.remove(0);
        }
        self.vec.push(value);
    }

    fn iter(&self) -> impl Iterator<Item = &T> {
        self.vec.iter()
    }

    fn len(&self) -> usize {
        self.vec.len()
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Clone)]
pub struct Table<T: PartialEq + Serialize + Clone + Identifiable> {
    id: Identifier,
    k: usize, // As defined by Kademlia
    map: BTreeMap<usize, Bucket<T>>,
}

impl<T: PartialEq + Serialize + Clone + Identifiable> Table<T> {
    pub fn new(id: Identifier, k: usize) -> Table<T> {
        let capacity: usize = id.id_size().into();
        Table {
            id,
            k,
            map: BTreeMap::new(),
        }
    }

    pub fn k_closest(&self) -> impl Iterator<Item = &T> {
        let k = self.k;
        self.map.values().flat_map(|bucket| bucket.iter()).take(k)
    }

    fn get_mut_or_insert(&mut self, distance: usize) -> &mut Bucket<T> {
        let k = self.k;
        self.map.entry(distance).or_insert(Bucket::new(k))
    }

    fn iter(&self) -> impl Iterator<Item = &Bucket<T>> {
        self.map.values()
    }

    // TODO: pretty sure this is wrong
    pub fn k_closest_to(&self, other_id: &Identifier) -> Vec<T> {
        self.map
            .range((&self.id) ^ other_id..)
            .map(|x| x.1)
            .flat_map(|bucket| bucket.iter())
            .take(self.k)
            .map(Clone::clone)
            .collect()
    }

    pub fn update<F>(&mut self, value: T, ping: F)
    where
        F: Fn(&T) -> bool,
    {
        let distance = &self.id ^ value.id();
        self.get_mut_or_insert(distance).update(value, ping);
    }

    pub fn insert(&mut self, value: T) {
        let distance = &self.id ^ value.id();
        self.get_mut_or_insert(distance).insert(value);
    }
}

impl<T: PartialEq + Serialize + Clone + Identifiable> Identifiable for Table<T> {
    fn id(&self) -> &Identifier {
        &self.id
    }

    fn id_size(&self) -> &IdentifierSize {
        &self.id.id_size()
    }
}

#[cfg(test)]
mod test {
    use super::super::id::test::{bits_id, one_id, zero_id};
    use super::{Bucket, Identifier, IdentifierSize, Table};
    mod bucket {
        use super::*;
        fn ping_succeeds(_: &i32) -> bool {
            true
        }
        fn ping_fails(_: &i32) -> bool {
            false
        }

        #[test]
        fn bucket_insert_stops_at_k_and_erases_older() {
            let mut bucket = Bucket::new(3);
            bucket.insert(1);
            bucket.insert(2);
            bucket.insert(3);
            bucket.insert(4);
            bucket.insert(5);
            assert_eq!(bucket.len(), 3);
            assert_eq!(bucket.vec, vec![3, 4, 5]);
        }

        #[test]
        fn bucket_update_stops_at_k_and_keeps_older_when_pings_succeed() {
            let mut bucket = Bucket::new(3);
            bucket.update(1, ping_succeeds);
            bucket.update(2, ping_succeeds);
            bucket.update(3, ping_succeeds);
            bucket.update(4, ping_succeeds);
            bucket.update(5, ping_fails);
            assert_eq!(bucket.len(), 3);
            assert_eq!(bucket.vec, vec![2, 3, 5]);
        }

        #[test]
        fn bucket_update_stops_at_k_and_removes_older_when_pings_fail() {
            let mut bucket = Bucket::new(3);
            bucket.update(1, ping_fails);
            bucket.update(2, ping_fails);
            bucket.update(3, ping_fails);
            bucket.update(4, ping_fails);
            assert_eq!(bucket.len(), 3);
            assert_eq!(bucket.vec, vec![2, 3, 4]);
        }
    }

    mod table {
        use super::*;
        use bit_vec::BitVec;

        fn table() -> Table<Identifier> {
            Table::new(zero_id(&IdentifierSize::default()), 1)
        }

        #[test]
        fn table_inserts_one_per_bucket() {
            let id_size = IdentifierSize::default();
            let len: usize = IdentifierSize::default().into();
            let mut table = table();
            id_size
                .as_range()
                .into_iter()
                .map(|x| {
                    println!("Val is {}", x);
                    bits_id(&id_size, BitVec::from_fn(len, |index| x - 1 == index))
                })
                .for_each(|id| table.insert(id));
            table.iter().for_each(|bucket| assert_eq!(bucket.len(), 1));
        }
    }
}
