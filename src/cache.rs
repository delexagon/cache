use std::{collections::HashMap, sync::Arc};
use lru::LruCache;
use parking_lot::{ArcRwLockReadGuard, ArcRwLockWriteGuard, Mutex, RawRwLock, RwLock};
use std::ops::{Deref, DerefMut};

struct RefReturn<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    k: K,
    cache: Arc<Mutex<CacheMutBase<K, V, CC>>>
}
impl<K, V, CC> Drop for RefReturn<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    fn drop(&mut self) {
        let mut cache = self.cache.lock();
        if !cache.active.get(&self.k).unwrap().1.is_locked() {
            let _ = cache.deactivate(&self.k);
        }
    }
}


pub struct CMRef<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    item: ArcRwLockReadGuard<RawRwLock, V>,
    _drop: RefReturn<K, V, CC>,
}
impl<'a, K, V, CC> Deref for CMRef<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    type Target = V;
    fn deref(&self) -> &Self::Target { self.item.deref() }
}
pub struct CMRefMut<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    item: ArcRwLockWriteGuard<RawRwLock, V>,
    _drop: RefReturn<K, V, CC>,
}
impl<'a, K, V, CC> Deref for CMRefMut<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    type Target = V;
    fn deref(&self) -> &Self::Target { self.item.deref() }
}
impl<'a, K, V, CC> DerefMut for CMRefMut<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.item.deref_mut() }
}

pub trait CacheCompatible<K, V> {
    type Error;

    fn contains(&self, k: K) -> bool;
    fn get(&mut self, k: K) -> Result<V, Self::Error>;
    /// Called when get from cache is finished. This is only required if the backend removes the v to pass to the cache.
    fn replace(&mut self, k: K, v: V);
}

pub trait CacheMutCompatible<K, V>: CacheCompatible<K, V> {
    fn insert(&mut self, k: K, v: V) -> Result<(), Self::Error>;
    fn remove(&mut self, k: K) -> Result<(), Self::Error>;
    /// Should ensure the cache resolves to a stable state. No active references will remain.
    /// For backends that do not have any notion of backing up, this would not be necessary.
    fn commit(&mut self) -> Result<(), Self::Error>;
}

pub struct CacheMutBase<K,V,CC> where
CC: CacheMutCompatible<K, V>, K: Copy+Eq+std::hash::Hash {
    compatible: CC, lru: LruCache<K, (bool, Arc<RwLock<V>>)>, active: HashMap<K, (bool, Arc<RwLock<V>>)>
} impl<K,V,CC> CacheMutBase<K,V,CC> where 
CC: CacheMutCompatible<K, V>, K: Copy+Eq+std::hash::Hash {
    fn new(compatible: CC, capacity: usize) -> Self {
        Self { compatible, lru: LruCache::new(std::num::NonZero::new(capacity).unwrap()), active: HashMap::new() }
    }
    fn insert(&mut self, k: K, v: V) -> Result<(), CC::Error> {
        if self.active.contains_key(&k) {
            panic!();
        } else if let Some((changed, vv)) = self.lru.get_mut(&k) {
            *changed = true;
            *vv = Arc::new(RwLock::new(v));
        } else {
            self.compatible.insert(k, v)?;
        }
        Ok(())
    }
    fn remove(&mut self, k: &K) -> Result<(), CC::Error> {
        if self.active.contains_key(&k) {
            panic!();
        }
        self.lru.pop(k);
        self.compatible.remove(*k)?;
        Ok(())
    }
    fn contains(&self, k: &K) -> bool {
        self.compatible.contains(*k) || self.active.contains_key(k) || self.lru.contains(k)
    }
    fn get(&mut self, k: &K) -> Result<ArcRwLockReadGuard<RawRwLock, V>, CC::Error> {
        if let Some((_, arc)) = self.active.get(k) {
            return Ok(arc.read_arc());
        } else if let Some(item) = self.lru.pop(k) {
            let arc = item.1.read_arc();
            self.active.insert(*k, item);
            return Ok(arc);
        } else {
            let v = self.compatible.get(*k)?;
            let arc = Arc::new(RwLock::new(v));
            let r = arc.read_arc();
            self.active.insert(*k, (false, arc));
            return Ok(r);
        }
    }
    fn get_mut(&mut self, k: &K) -> Result<ArcRwLockWriteGuard<RawRwLock, V>, CC::Error> {
        if let Some(_) = self.active.get(k) {
            panic!();
        } else if let Some((_, v)) = self.lru.pop(k) {
            let arc = v.write_arc();
            self.active.insert(*k, (true, v));
            return Ok(arc);
        } else {
            let v = self.compatible.get(*k)?;
            let arc = Arc::new(RwLock::new(v));
            let r = arc.write_arc();
            self.active.insert(*k, (true, arc));
            return Ok(r);
        }
    }
    fn commit(&mut self) -> Result<(), CC::Error> {
        if self.active.len() > 0 {
            panic!();
        }
        while let Some((k, (changed, v))) = self.lru.pop_lru() {
            let v = Arc::try_unwrap(v).unwrap_or_else(|_| unreachable!()).into_inner();
            if changed {
                self.compatible.insert(k, v)?;
            } else {
                self.compatible.replace(k, v);
            }
        }
        self.compatible.commit()?;
        Ok(())
    }
    fn deactivate(&mut self, k: &K) -> Result<(), CC::Error> {
        let Some(item) = self.active.remove(k) else {return Ok(())};
        let out = self.lru.push(*k, item);
        if let Some((k, (changed, v))) = out {
            let v = Arc::try_unwrap(v).unwrap_or_else(|_| unreachable!()).into_inner();
            if changed {
                self.compatible.insert(k, v)?;
            } else {
                self.compatible.replace(k, v);
            }
        }
        Ok(())
    }
    fn cap(&self) -> usize { self.lru.cap().into() }
    fn active(&self, k: &K) -> bool { self.active.contains_key(&k) }
    fn num_active(&self) -> usize { self.active.len() }
}
impl<K, V, CC> Drop for CacheMutBase<K, V, CC> where 
K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K,V> {
    fn drop(&mut self) {
        let _ = self.commit();
    }
}

#[derive(Clone)]
pub struct CacheMut<K, V, CC>(Arc<Mutex<CacheMutBase<K, V, CC>>>) where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V>;
impl<K, V, CC> CacheMut<K, V, CC> where K: Copy+Eq+std::hash::Hash, CC: CacheMutCompatible<K, V> {
    pub fn new(compatible: CC, capacity: usize) -> Self {
        Self(Arc::new(Mutex::new(CacheMutBase::new(compatible, capacity))))
    }
    pub fn insert(&mut self, k: K, v: V) -> Result<(), CC::Error> { self.0.lock().insert(k, v) }
    pub fn remove(&mut self, k: &K) -> Result<(), CC::Error> { self.0.lock().remove(k) }
    pub fn contains(&self, k: &K) -> bool { self.0.lock().contains(k) }
    pub fn get(&self, k: &K) -> Result<CMRef<K, V, CC>, CC::Error> {
        self.0.lock().get(k).map(|v|
            CMRef { item: v, _drop: RefReturn { k: *k, cache: self.0.clone() } }
        )
    }
    pub fn get_mut(&self, k: &K) -> Result<CMRefMut<K, V, CC>, CC::Error> {
        self.0.lock().get_mut(k).map(|v|
            CMRefMut { item: v, _drop: RefReturn { k: *k, cache: self.0.clone() } }
        )
    }
    pub fn commit(&mut self) -> Result<(), CC::Error> { self.0.lock().commit() }
    pub fn cap(&self) -> usize { self.0.lock().cap() }
    pub fn active(&self, k: &K) -> bool { self.0.lock().active(k) }
    pub fn num_active(&self) -> usize { self.0.lock().num_active() }
}
