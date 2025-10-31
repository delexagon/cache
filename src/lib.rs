mod cache;
pub mod folder_compatible;
pub mod hashmap_compatible;
pub use cache::{CMRef, CMRefMut, CacheMut, CacheCompatible, CacheMutCompatible};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::hashmap_compatible::NotInMap;
    use crate::CacheMut;

    #[test]
    fn get() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }
        let mut cache = CacheMut::new(map, 4);
        let get = cache.get(&3).unwrap();
        assert_eq!(*get, "3");
    }
    
    #[test]
    fn get_mut() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }
        let mut cache = CacheMut::new(map, 4);
        
        {
            let mut get_mut = cache.get_mut(&3).unwrap();
            get_mut.push_str("_modified");
        }

        let get = cache.get(&3).unwrap();
        assert_eq!(*get, "3_modified");
    }

    #[test]
    fn get_mut_two_concurrent() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }

        let mut cache = CacheMut::new(map, 4);

        // Get two mutable references from the cache concurrently.
        let mut get_mut1 = cache.get_mut(&2).unwrap();
        let mut get_mut2 = cache.get_mut(&5).unwrap();

        // Mutate both values.
        get_mut1.push_str("_a");
        get_mut2.push_str("_b");

        // Drop them so we can borrow again immutably.
        drop(get_mut1);
        drop(get_mut2);

        // Verify both mutations persisted.
        let get1 = cache.get(&2).unwrap();
        let get2 = cache.get(&5).unwrap();

        assert_eq!(*get1, "2_a");
        assert_eq!(*get2, "5_b");
    }

    #[test]
    fn get_then_get_mut_inactive() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }

        let mut cache = CacheMut::new(map, 4);

        // Get an immutable reference
        {
            let get_ref = cache.get(&2).unwrap();
            assert_eq!(*get_ref, "2");
            // While active, it should be marked as active
            assert!(cache.active(&2));
        } // dropped here

        // Get a mutable reference
        {
            let mut get_mut_ref = cache.get_mut(&5).unwrap();
            get_mut_ref.push_str("_edited");
            // While active, it should be marked as active
            assert!(cache.active(&5));
        } // dropped here

        // After both borrows are dropped, both keys should be inactive
        assert!(!cache.active(&2));
        assert!(!cache.active(&5));

        // Verify that the modification persisted
        let get_ref = cache.get(&5).unwrap();
        assert_eq!(*get_ref, "5_edited");
    }

    #[test]
    fn get_twice_drop_one_still_active() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }

        let mut cache = CacheMut::new(map, 4);

        // First immutable get
        let get1 = cache.get(&3).unwrap();

        // Second immutable get to the same key
        let get2 = cache.get(&3).unwrap();

        // Both borrows are active, so the entry should be marked active
        assert!(cache.active(&3));

        // Drop one of the gets
        drop(get1);

        // The other get is still alive, so it should still be marked active
        assert!(cache.active(&3));

        // Drop the last reference
        drop(get2);

        // Now that all borrows are gone, the entry should no longer be active
        assert!(!cache.active(&3));
    }

    #[test]
    fn modified_value_persists_after_cache_eviction() {
        let mut map = HashMap::new();
        for i in 0..10 {
            map.insert(i, i.to_string());
        }

        // Cache capacity = 4
        let mut cache = CacheMut::new(map, 4);

        // Modify one value
        {
            let mut val = cache.get_mut(&8).unwrap();
            val.push_str("_changed");
        }

        // Access more than the cache capacity (0..5 = 5 unique keys)
        for i in 0..5 {
            let _ = cache.get(&i).unwrap();
        }

        // Ensure that the modified value was not lost or overwritten
        let val = cache.get(&8).unwrap();
        assert_eq!(*val, "8_changed");

        // Also, ensure that the value is still accessible
        assert!(!val.is_empty());
    }

    #[test]
    fn get_missing_returns_not_in_map() {
        let mut map = HashMap::new();
        for i in 0..5 {
            map.insert(i, i.to_string());
        }

        let mut cache = CacheMut::new(map, 4);

        let result = cache.get(&99);

        assert!(match result {Ok(_) => false, Err(NotInMap) => true});
    }

    #[test]
    fn insert_adds_new_value() {
        let map = HashMap::new();
        let mut cache = CacheMut::new(map, 4);

        // Insert a new key/value pair
        cache.insert(10, "ten".to_string()).unwrap();

        // Should be retrievable
        let val = cache.get(&10).unwrap();
        assert_eq!(*val, "ten");
    }

    #[test]
    fn remove_deletes_value() {
        let mut map = HashMap::new();
        for i in 0..5 {
            map.insert(i, i.to_string());
        }
        let mut cache = CacheMut::new(map, 4);

        // Confirm the value exists
        assert_eq!(*cache.get(&3).unwrap(), "3");

        // Remove it
        cache.remove(&3).unwrap();

        // Should now be gone
        let result = cache.get(&3);
        assert!(match result {Ok(_) => false, Err(NotInMap) => true});
    }

    #[test]
    fn remove_after_get_prevents_future_access() {
        let mut map = HashMap::new();
        for i in 0..5 {
            map.insert(i, i.to_string());
        }
        let mut cache = CacheMut::new(map, 4);

        // Get an item (loads it into cache)
        let val = cache.get(&2).unwrap();
        assert_eq!(*val, "2");
        drop(val);

        // Remove it from the map while it’s still cached
        cache.remove(&2).unwrap();

        // The cache should now treat it as gone — even if it was in cache memory
        let result = cache.get(&2);
        assert!(match result {Ok(_) => false, Err(NotInMap) => true});
    }
}

#[cfg(test)]
mod folder_tests {
    use crate::CacheMut;
    use crate::folder_compatible::FolderCache;
    use tempdir::TempDir;

    #[test]
    fn insert() {
        let tempdir = TempDir::new("test").unwrap();
        let folder = FolderCache::continued(tempdir.into_path()).unwrap();
        let mut cache = CacheMut::new(folder, 2);
        for i in 0..10 {
            cache.insert(i, i.to_string()).unwrap();
        }
        assert_eq!(*cache.get(&1).unwrap(), "1");
        assert_eq!(*cache.get(&2).unwrap(), "2");
        assert_eq!(*cache.get(&3).unwrap(), "3");
        assert_eq!(*cache.get(&4).unwrap(), "4");
        assert_eq!(*cache.get(&9).unwrap(), "9");
        assert_eq!(*cache.get(&1).unwrap(), "1");
    }

    #[test]
    fn persist_across_drop() {
        // Create a temporary folder for persistence
        let tempdir = TempDir::new("test_persist").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        {
            // Create a cache and insert a few values
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache = CacheMut::new(folder, 2);
            cache.insert(42, "meaning".to_string()).unwrap();
            cache.insert(99, "bottles".to_string()).unwrap();

            // Ensure value is get()table before drop
            assert_eq!(*cache.get(&42).unwrap(), "meaning");
        } // <- cache and folder dropped here

        // Continue the cache from the same folder path
        {
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 2);

            // Ensure persisted value is still available
            assert_eq!(*cache.get(&42).unwrap(), "meaning");
            assert_eq!(*cache.get(&99).unwrap(), "bottles");
        }
    }


    #[test]
    fn cleared_removes_old_data() {
        // Create a temporary directory and persist some data
        let tempdir = TempDir::new("test_cleared").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        {
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache = CacheMut::new(folder, 2);
            cache.insert(1, "one".to_string()).unwrap();
            cache.insert(2, "two".to_string()).unwrap();
            assert_eq!(*cache.get(&1).unwrap(), "one");
        }

        // Check that files exist before clearing
        let entries_before: Vec<_> = std::fs::read_dir(&folder_path)
            .unwrap()
            .collect();
        assert!(!entries_before.is_empty(), "Folder should not be empty before clearing");

        // Recreate using cleared() — should delete old data
        {
            let folder = FolderCache::cleared(folder_path.clone()).unwrap();

            // Folder should now be empty
            let entries_after: Vec<_> = std::fs::read_dir(&folder_path)
                .unwrap()
                .collect();
            assert!(entries_after.is_empty(), "Folder should be empty after FolderCache::cleared");

            let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 2);

            // Old values should not exist
            assert!(cache.get(&1).is_err(), "Old value 1 should not persist after clear");
            assert!(cache.get(&2).is_err(), "Old value 2 should not persist after clear");
        }
    }

    #[test]
    fn mutation_persists_after_eviction() {
        // Create persistent folder
        let tempdir = TempDir::new("test_mut_persist").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        // Create a cache with small capacity so eviction happens quickly
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 2);

        // Insert several items
        for i in 0..5 {
            cache.insert(i, format!("value_{i}")).unwrap();
        }

        // Mutate a value via get_mut
        {
            let mut v = cache.get_mut(&1).unwrap();
            *v = "modified".to_string();
        }

        // Cause eviction: hold more than capacity (2) active entries
        let _a = cache.get(&2).unwrap();
        let _b = cache.get(&3).unwrap();
        let _c = cache.get(&4).unwrap();
        // The mutated value (key 1) should be evicted here

        // Drop active references to allow reloading
        drop(_a);
        drop(_b);
        drop(_c);

        // Fetch the value again (forcing reload from folder)
        let v = cache.get(&1).unwrap();

        // The mutation should have persisted through eviction
        assert_eq!(*v, "modified");
    }

    #[test]
    fn commit_persists_non_evicted_mutations() {
        // Create a temporary persistent folder
        let tempdir = TempDir::new("test_commit_persist").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        // Create a cache with capacity large enough to hold all entries
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 10);

        // Insert a few values
        for i in 0..5 {
            cache.insert(i, format!("value_{i}")).unwrap();
        }

        // Mutate several values through get_mut()
        {
            *cache.get_mut(&0).unwrap() = "zero".to_string();
            *cache.get_mut(&1).unwrap() = "one".to_string();
            *cache.get_mut(&2).unwrap() = "two".to_string();
        }

        // No eviction occurs because capacity is large enough
        // Force all in-memory changes to persist
        cache.commit().unwrap();

        // Drop the cache entirely to ensure reload from disk
        drop(cache);

        // Reload from the same folder and verify persistence
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 10);

        assert_eq!(*cache.get(&0).unwrap(), "zero");
        assert_eq!(*cache.get(&1).unwrap(), "one");
        assert_eq!(*cache.get(&2).unwrap(), "two");
        assert_eq!(*cache.get(&3).unwrap(), "value_3");
        assert_eq!(*cache.get(&4).unwrap(), "value_4");
    }

    #[test]
    fn many_insert() {
        let tempdir = TempDir::new("test").unwrap();
        let folder_path = tempdir.path().to_path_buf();
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 2);
        for i in 0..200 {
            cache.insert(i, i.to_string()).unwrap();
        }
        drop(cache);
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 2);
        for i in 0..200 {
            assert_eq!(*cache.get(&i).unwrap(), i.to_string());
        }
    }

    #[test]
    fn variable_size_insert() {
        let tempdir = TempDir::new("test_many_insert").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        // Create a folder-backed cache with small capacity to trigger evictions
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 2);

        // Insert 200 entries, each containing "memphis" repeated i times
        for i in 0..200 {
            let value = "memphis".repeat(i);
            cache.insert(i, value).unwrap();
        }

        // Drop cache to force write-back to folder
        drop(cache);

        // Recreate from the same folder and verify persistence
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache: CacheMut<usize, String, FolderCache<usize>> = CacheMut::new(folder, 2);

        for i in 0..200 {
            let expected = "memphis".repeat(i);
            assert_eq!(*cache.get(&i).unwrap(), expected);
        }
    }

    #[test]
    fn mutate_short_to_long_persists() {
        // Create a temporary folder for persistence
        let tempdir = TempDir::new("test_mutate_short_to_long").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        // Initialize the cache
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 2);

        // Insert a short string
        cache.insert(1, "hi".to_string()).unwrap();
        assert_eq!(*cache.get(&1).unwrap(), "hi");

        // Mutate it to a long string
        {
            let mut v = cache.get_mut(&1).unwrap();
            *v = "memphis".repeat(500); // very long string
        }

        // Drop to force persistence to disk
        drop(cache);

        // Reload the cache from the same folder
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 2);

        // Ensure the long string persisted correctly
        let value = cache.get(&1).unwrap();
        assert_eq!(value.len(), "memphis".len() * 500);
        assert!(value.starts_with("memphis"));
        assert!(value.ends_with("memphis"));
    }

    #[test]
    fn file_reuse_and_size_stability() {
        // Create a persistent folder
        let tempdir = TempDir::new("test_file_reuse").unwrap();
        let folder_path = tempdir.path().to_path_buf();

        // Create a cache and insert 0..7
        let folder = FolderCache::continued(folder_path.clone()).unwrap();
        let mut cache = CacheMut::new(folder, 4);

        for i in 0..7 {
            cache.insert(i, i.to_string()).unwrap();
        }

        // Drop cache to flush all data
        drop(cache);

        // Ensure there is only one .cache file
        let mut cache_files: Vec<_> = std::fs::read_dir(&folder_path)
            .unwrap()
            .filter_map(|e| {
                let entry = e.unwrap();
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "cache") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            cache_files.len(),
            1,
            "Expected exactly one .cache file in folder"
        );

        let cache_file = cache_files.pop().unwrap();
        let initial_size = std::fs::metadata(&cache_file).unwrap().len();

        // Reopen cache and remove all inserted values
        {
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 4);

            for i in 0..7 {
                cache.remove(&i).unwrap();
            }
        }

        // Drop and restore — ensure all values are gone
        {
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache: CacheMut<i32, String, FolderCache<i32>> = CacheMut::new(folder, 4);

            for i in 0..7 {
                assert!(cache.get(&i).is_err(), "Expected key {i} to be absent");
            }
        }

        // Insert 0..7 again, but values are reversed (7 - i).to_string()
        {
            let folder = FolderCache::continued(folder_path.clone()).unwrap();
            let mut cache = CacheMut::new(folder, 4);

            for i in 0..7 {
                cache.insert(i, (7 - i).to_string()).unwrap();
            }

            drop(cache);
        }
        
        let v: Vec<_> = std::fs::read_dir(&folder_path)
            .unwrap()
            .filter_map(|e| {
                let entry = e.unwrap();
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "cache") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(v.len(), 1);

        // Check .cache file size again — should not have grown
        let final_size = std::fs::metadata(&cache_file).unwrap().len();

        assert_eq!(
            initial_size, final_size,
            ".cache file size should remain the same after reinsertion"
        );
    }
}
