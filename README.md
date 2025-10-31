# Cache
This library provides a filesystem cache to Rust. It requires serde to store the keys and values. Cache has two functions: First, it automatically stores objects into the filesystem so that only N most at a time are present. Second,  get() and get_mut() calls may retrieve data from the filesystem, adding it to the cache but maintaining currently taken references. Both get() and get_mut() only need an 'immutable' cache, but insertion and removal still require a mutable reference.
## Usage notes:
- Cache currently only supports 'online' caching; that is, objects are dropped into the filesystem directly once dropped. This means that if the cache does not close manually, objects which have been modified and dropped from the cache will be updated and objects which have been modified and not dropped will not be. This means using the cache mutably will need to periodically create a backup in case it unexpectedly drops and breaks.
## TODO:
- This crate will become much more flexible when the logic of the cache is separated from the overall filesystem structure, with the necessary functions being placed into a trait. This will almost certainly be necessary to implement for Rain's save files to work in web assembly.
- Needs multithreaded support.
- Should support a more traditional structure with SQL style 'commits', where all modified objects are periodically updated.