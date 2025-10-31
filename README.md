# Cache
This library creates a Rust cache. It keeps elements alive, but limits the amount kept at a time. Cache operations are done with Arc<RwLock<>>, meaning it could theoretically be multithreaded, but I haven't made thread lock functions for it so it's not practical. The cache functionality is split into backend and frontend to make it flexible in terms of storage.
## Usage
To make a backend, implement the following traits:  
```
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
```
This can be turned into a cache as so:  
`let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 2);`
where FolderCache<V> is the pre-initialized struct with the CacheCompatible and CacheMutCompatible traits.  
The cache allows the online viewing of items in the backend through the functions:  
```
fn insert(&mut self, k: K, v: V) -> Result<(), CC::Error>
fn remove(&mut self, k: &K) -> Result<(), CC::Error>
fn contains(&self, k: &K) -> bool
fn get(&self, k: &K) -> Result<CMRef<K, V, CC>, CC::Error>
fn get_mut(&self, k: &K) -> Result<CMRefMut<K, V, CC>, CC::Error>
fn commit(&mut self) -> Result<(), CC::Error>
fn active(&self, k: &K) -> bool
fn num_active(&self) -> usize
```
Note that references retrieved from the cache have no lifespan. The cache will only close (storing all items) when itself and all references are out of scope.  
Also included is the FolderCache in the `folder_compatible` subsection, which sets up a cache in a folder if both key and value are serde-compatible.
## TODO
- Folder cache should have actual commit behavior
- Commit should be possible when items are active
- Make multithread locking functions
- Add a couple more backends (sqlite?)
- Make try_x functions for non-panicking function variants
