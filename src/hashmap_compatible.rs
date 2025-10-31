use std::collections::HashMap;
use crate::{CacheCompatible, CacheMutCompatible};

#[derive(Debug, PartialEq, Eq)]
pub struct NotInMap;

impl<K, V> CacheCompatible<K, V> for HashMap<K, V> where K: Eq+std::hash::Hash, {
    type Error = NotInMap;

    fn contains(&self, k: K) -> bool {
        self.contains_key(&k)
    }

    fn get(&mut self, k: K) -> Result<V, Self::Error> {
        match HashMap::<K,V>::remove(self, &k) {
            Some(v) => Ok(v),
            None => Err(NotInMap)
        }
    }

    fn replace(&mut self, k: K, v: V) {
        self.insert(k, v);
    }
}

impl<K, V> CacheMutCompatible<K, V> for HashMap<K, V> where K: Eq+std::hash::Hash {
    fn insert(&mut self, k: K, v: V) -> Result<(), Self::Error> {
        HashMap::<K,V>::insert(self, k, v);
        Ok(())
    }

    fn remove(&mut self, k: K) -> Result<(), Self::Error> {
        HashMap::<K,V>::remove(self, &k);
        Ok(())
    }

    fn commit(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
