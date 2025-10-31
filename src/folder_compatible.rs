use std::path::{Path,PathBuf};
use std::fs::{OpenOptions,File};
use serde::{Serialize,Deserialize};
use std::collections::HashMap;
use std::io::{Read,Write,Seek,SeekFrom};
use std::ffi::OsStr;
use std::mem::size_of;
use rmp_serde;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FolderCacheError {
    #[error("decode error: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("encode error: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Not present in cache")]
    Nothing
}

use crate::{CacheCompatible, CacheMutCompatible};

const EXTENSION: &str = "cache";

#[derive(Clone,Copy)]
struct CacheLevel1 {num_items: u64, size_per_item: u64, reserved: u64}
impl Ord for CacheLevel1 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {self.size_per_item.cmp(&other.size_per_item)}
} impl PartialOrd for CacheLevel1 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {Some(self.cmp(other))}
}
impl PartialEq for CacheLevel1 {
    fn eq(&self, other: &Self) -> bool {self.size_per_item == other.size_per_item}
}
impl Eq for CacheLevel1 {}
impl CacheLevel1 {
    fn create_file(&mut self, folder: &Path) -> Result<PathBuf, FolderCacheError> {
        let path = folder.join(self.file_name());
        let mut file = OpenOptions::new().create(true).write(true).open(&path)?;
        self.num_items = 0;
        Level1Header(self.num_items).raw_write(&mut file)?;
        file.set_len(Level1Header::BYTES+self.reserved*self.size_per_item)?;
        return Ok(path);
    }
    fn from_path(path: &Path) -> Result<Option<Self>, FolderCacheError> {
        fn read_name(name: Option<&OsStr>) -> Option<u64> {
            let mut name_itr = name?.to_str()?.split('.');
            let ret = name_itr.next()?.parse().ok()?;
            if name_itr.next()? == EXTENSION {Some(ret)} else {None}
        }
        if let Some(size_per_item) = read_name(path.file_name()) {
            let length = path.metadata()?.len();
            let mut filep = OpenOptions::new().read(true).open(path)?;
            let Level1Header(num_items) = Level1Header::raw_read(&mut filep)?;
            return Ok(Some(CacheLevel1 {size_per_item, num_items, reserved: (length-Level1Header::BYTES)/size_per_item}));
        } else {
            return Ok(None);
        }
    }
    fn rewrite_header(&self, filep: &mut File) -> Result<(), FolderCacheError> {
        filep.seek(SeekFrom::Start(0))?;
        Level1Header(self.num_items).raw_write(filep)?;
        Ok(())
    }
    fn len(&self) -> usize {self.num_items as usize}
    fn read_k<K: for <'a> Deserialize<'a>>(&self, filep: &mut File, i: u64) -> Result<K, FolderCacheError> {
        filep.seek(SeekFrom::Start(Level1Header::BYTES+i*self.size_per_item))?;
        let Level1EntryHeader(k_size, v_size) = Level1EntryHeader::raw_read(filep)?;
        filep.seek(SeekFrom::Start(Level1Header::BYTES+i*self.size_per_item+Level1EntryHeader::BYTES+v_size))?;
        let mut read = vec![0; k_size as usize];
        filep.read(&mut read)?;
        let k = rmp_serde::from_slice(&read[0..k_size as usize])?;
        return Ok(k);
    }
    fn read_v<V: for <'a> Deserialize<'a>>(&self, filep: &mut File, i: u64) -> Result<V, FolderCacheError> {
        filep.seek(SeekFrom::Start(Level1Header::BYTES+i*self.size_per_item))?;
        let Level1EntryHeader(_, v_size) = Level1EntryHeader::raw_read(filep)?;
        let mut read = vec![0; v_size as usize];
        filep.read(&mut read)?;
        let v = rmp_serde::from_slice(&read[0..v_size as usize])?;
        return Ok(v);
    }
    #[allow(dead_code)]
    fn read<K: for <'a> Deserialize<'a>, V: for <'a> Deserialize<'a>>(&self, filep: &mut File, i: u64) -> Result<(K,V), FolderCacheError> {
        filep.seek(SeekFrom::Start(Level1Header::BYTES+i*self.size_per_item))?;
        let Level1EntryHeader(k_size, v_size) = Level1EntryHeader::raw_read(filep)?;
        let mut read = vec![0; k_size as usize+v_size as usize];
        filep.read(&mut read)?;
        let v = rmp_serde::from_slice(&read[0..v_size as usize])?;
        let k = rmp_serde::from_slice(&read[v_size as usize..k_size as usize+v_size as usize])?;
        return Ok((k,v));
    }
    /// Removes by swapping. If something was swapper, returns the K that was swapped into the position i.
    fn swap_remove<K: for <'a> Deserialize<'a>>(&mut self, filep: &mut File, i: u64) -> Result<Option<K>, FolderCacheError> {
        if i == self.num_items-1 {
            self.num_items -= 1;
            self.rewrite_header(filep)?;
            return Ok(None);
        } else {
            let mut read = vec![0; self.size_per_item as usize];
            filep.seek(SeekFrom::Start(Level1Header::BYTES+self.size_per_item*(self.num_items-1)))?;
            filep.read(&mut read)?;
            let Level1EntryHeader(k_size, v_size) = Level1EntryHeader::from_bytes(&read[0..Level1EntryHeader::BYTES as usize]);
            let k = rmp_serde::from_slice(&read[Level1EntryHeader::BYTES as usize+v_size as usize..Level1EntryHeader::BYTES as usize+k_size as usize+v_size as usize])?;
            filep.seek(SeekFrom::Start(Level1Header::BYTES+self.size_per_item*i))?;
            filep.write(&read)?;
            self.num_items -= 1;
            self.rewrite_header(filep)?;
            return Ok(Some(k));
        }
    }
    #[allow(dead_code)]
    fn clear(&mut self, filep: &mut File) -> Result<(), FolderCacheError> {
        self.num_items = 0;
        self.rewrite_header(filep)?;
        Ok(())
    }
    fn add(&mut self, filep: &mut File, kser: Vec<u8>, vser: Vec<u8>) -> Result<u64, FolderCacheError> {
        if self.num_items >= self.reserved {
            filep.set_len(Level1Header::BYTES+self.reserved*2*self.size_per_item)?;
            self.reserved *= 2;
        }
        filep.seek(SeekFrom::Start(Level1Header::BYTES+self.num_items*self.size_per_item))?;
        Level1EntryHeader(kser.len() as u64, vser.len() as u64).raw_write(filep)?;
        filep.write(&vser)?;
        filep.write(&kser)?;
        self.num_items += 1;
        self.rewrite_header(filep)?;
        return Ok(self.num_items-1);
    }
    fn overwrite(&mut self, filep: &mut File, i: u64, kser: Vec<u8>, vser: Vec<u8>) -> Result<(), FolderCacheError> {
        filep.seek(SeekFrom::Start(Level1Header::BYTES+i*self.size_per_item))?;
        Level1EntryHeader(kser.len() as u64, vser.len() as u64).raw_write(filep)?;
        filep.write(&vser)?;
        filep.write(&kser)?;
        Ok(())
    }
    fn file_name(&self) -> PathBuf {
        return PathBuf::from(format!("{}.{}", self.size_per_item, EXTENSION));
    }
}
const SZU64: usize = size_of::<u64>();
struct Level1Header(u64);
struct Level1EntryHeader(u64,u64);
impl Level1Header {
    const BYTES: u64 = size_of::<u64>() as u64;
    fn raw_write(&self, file: &mut File) -> Result<(), FolderCacheError> {
        let bytes = self.to_bytes();
        file.write(&bytes)?;
        Ok(())
    }
    fn raw_read(file: &mut File) -> Result<Self, FolderCacheError> {
        let mut bytes = [0; Self::BYTES as usize];
        file.read(&mut bytes)?;
        Ok(Self::from_bytes(&bytes))
    }

    fn to_bytes(&self) -> [u8; Self::BYTES as usize] {self.0.to_le_bytes()}
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut a = [0; SZU64];
        a.copy_from_slice(&bytes[0..SZU64]);
        Self(u64::from_le_bytes(a))
    }
}
impl Level1EntryHeader {
    const BYTES: u64 = size_of::<u64>() as u64*2;
    fn raw_write(&self, file: &mut File) -> Result<(), FolderCacheError> {
        let bytes = self.to_bytes();
        file.write(&bytes)?;
        Ok(())
    }
    fn raw_read(file: &mut File) -> Result<Self, FolderCacheError> {
        let mut bytes = [0; Self::BYTES as usize];
        file.read(&mut bytes)?;
        Ok(Self::from_bytes(&bytes))
    }

    fn to_bytes(&self) -> [u8; Self::BYTES as usize] {
        let mut x: [u8; 16] = [0;Self::BYTES as usize];
        x[0..SZU64].copy_from_slice(&self.0.to_le_bytes());
        x[SZU64..SZU64*2].copy_from_slice(&self.1.to_le_bytes());
        return x;
    }
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut a = [0; SZU64];
        let mut b = [0; SZU64];
        a.copy_from_slice(&bytes[0..SZU64]);
        b.copy_from_slice(&bytes[SZU64..SZU64*2]);
        Self(u64::from_le_bytes(a), u64::from_le_bytes(b))
    }
}

fn foremost_bit(x: u64) -> u32 {
    for i in 0..64 {
        if (!((1<<i)-1)&x) == 0 {
            return i;
        }
    }
    return 64;
}
#[derive(Eq,PartialEq,Clone,Copy,Hash)]
struct Ref {file: u64, index: u64}
struct CacheLevel2 {files: Vec<CacheLevel1>, open: Option<(u64, File)>}
impl CacheLevel2 {
    fn new(folder: &Path) -> Result<Self, FolderCacheError> {
        let mut files = Vec::new();
        for file in folder.read_dir()? {
            if let Some(cachefile) = CacheLevel1::from_path(&file?.path())? {
                files.push(cachefile);
            }
        }
        files.sort();
        Ok(Self {files, open: None})
    }
    fn new_file(&mut self, folder: &Path, size_per_item: u64) -> Result<(), FolderCacheError> {
        let mut lvl1 = CacheLevel1 {num_items: 0, size_per_item, reserved: 4};
        if let Err(insertion_point) = self.files.binary_search(&lvl1) {
            lvl1.create_file(folder)?;
            self.files.insert(insertion_point, lvl1);
        }
        Ok(())
    }
    fn switch_open(&mut self, folder: &Path, size_per_item: u64) -> Result<usize, FolderCacheError> {
        if let Some((old_size, _)) = &mut self.open {
            if size_per_item == *old_size {return Ok(self.files.binary_search(&CacheLevel1 {reserved: 0, num_items: 0, size_per_item}).unwrap())}
            self.open = None;
        }
        match self.files.binary_search(&CacheLevel1 {num_items: 0, size_per_item, reserved: 0}) {
            Ok(i) => {
                let file = OpenOptions::new()
                .read(true).write(true).open(
                    folder.join(self.files[i].file_name())
                )?;
                self.open = Some((size_per_item, file));
                return Ok(i);
            },
            Err(i) => {
                self.new_file(folder, size_per_item)?;
                let file = OpenOptions::new()
                .read(true).write(true).open(
                    folder.join(self.files[i].file_name())
                )?;
                self.open = Some((size_per_item, file));
                return Ok(i);
            }
        }
    }
    fn load_to_hashmap<K: Eq+std::hash::Hash+for <'a> Deserialize<'a>>(&mut self, folder: &Path, map: &mut HashMap<K, Ref>) -> Result<(), FolderCacheError> {
        if let Some(_) = &mut self.open {
            self.open = None;
        }
        for filen in 0..self.files.len() {
            let mut filep = OpenOptions::new().read(true).open(folder.join(self.files[filen].file_name()))?;
            for i in 0..self.files[filen].len() as u64 {
                let k = self.files[filen].read_k(&mut filep, i)?;
                map.insert(k, Ref {file: self.files[filen].size_per_item, index: i});
            }
        }
        Ok(())
    }
    #[allow(dead_code)]
    fn get_v_against_k<K: for <'a> Deserialize<'a>+Eq,V: for <'a> Deserialize<'a>>(&mut self, folder: &Path, k: &K, refs: &[Ref]) -> Result<Option<V>, FolderCacheError> {
        for Ref {file, index} in refs {
            let i = self.switch_open(folder, *file)?;
            let (_, open) = self.open.as_mut().unwrap();

            let (test_k,v) = self.files[i].read::<K,V>(open, *index)?;
            if k == &test_k {
                return Ok(Some(v));
            }
        }
        return Ok(None);
    }
    fn get_v<V: for <'a> Deserialize<'a>>(&mut self, folder: &Path, Ref {file, index}: Ref) -> Result<V, FolderCacheError> {
        let i = self.switch_open(folder, file)?;
        let (_, open) = self.open.as_mut().unwrap();

        return self.files[i].read_v::<V>(open, index);
    }
    #[allow(dead_code)]
    fn get<K: for <'a> Deserialize<'a>,V: for <'a> Deserialize<'a>>(&mut self, folder: &Path, Ref {file, index}: Ref) -> Result<(K,V), FolderCacheError> {
        let i = self.switch_open(folder, file)?;
        let (_, open) = self.open.as_mut().unwrap();

        return self.files[i].read::<K,V>(open, index);
    }
    /// Returns the reference that was put IN PLACE of the old reference.
    fn remove<K: for <'a> Deserialize<'a>>(&mut self, folder: &Path, Ref {file, index}: Ref) -> Result<Option<K>, FolderCacheError> {
        let i = self.switch_open(folder, file)?;
        let (_, open) = self.open.as_mut().unwrap();
        return Ok(self.files[i].swap_remove(open, index)?);
    }
    fn add<K: Serialize, V: Serialize>(&mut self, folder: &Path, k: &K, v: &V) -> Result<Ref, FolderCacheError> {
        let kser = rmp_serde::encode::to_vec(k)?;
        let vser = rmp_serde::encode::to_vec(v)?;
        let full_len = kser.len() as u64+vser.len() as u64+Level1EntryHeader::BYTES;
        let file = 1<<(foremost_bit(full_len) as u64+1);
        let i = self.switch_open(folder, file)?;
        let (_, open) = self.open.as_mut().unwrap();
        let index = self.files[i].add(open, kser, vser)?;
        Ok(Ref { file, index })
    }
    fn overwrite<K: Serialize+for<'a> Deserialize<'a>, V: Serialize>(&mut self, folder: &Path, old_ref: Ref, k: &K, v: &V) -> Result<Option<(Option<K>, Ref)>, FolderCacheError> {
        let kser = rmp_serde::encode::to_vec(k)?;
        let vser = rmp_serde::encode::to_vec(v)?;
        let full_len = kser.len() as u64+vser.len() as u64+Level1EntryHeader::BYTES;
        let file = 1<<(foremost_bit(full_len) as u64+1);
        if file == old_ref.file {
            let i = self.switch_open(folder, file)?;
            let (_, open) = self.open.as_mut().unwrap();
            self.files[i].overwrite(open, old_ref.index, kser, vser)?;
            return Ok(None);
        } else {
            let i = self.switch_open(folder, old_ref.file)?;
            let (_, open) = self.open.as_mut().unwrap();
            let replace_ref = self.files[i].swap_remove(open, old_ref.index)?;
            let i = self.switch_open(folder, file)?;
            let (_, open) = self.open.as_mut().unwrap();
            let new_ref = Ref {file, index: self.files[i].add(open, kser, vser)?};
            return Ok(Some((replace_ref, new_ref)));
        }
    }
}

pub fn clear_cache(folder: &Path) -> Result<(), FolderCacheError> {
    for file in folder.read_dir()? {
        let path = file?.path();
        if let Some(_) = CacheLevel1::from_path(&path)? {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub struct FolderCache<K: std::hash::Hash+Eq+Serialize+for <'a> Deserialize<'a>>
{lvl2: CacheLevel2, map: HashMap<K, Ref>, folder: PathBuf}
impl<K> FolderCache<K> where
K: Eq+std::hash::Hash+Serialize+for <'a> Deserialize<'a> {
    pub fn cleared(folder: PathBuf) -> Result<Self, FolderCacheError> {
        clear_cache(&folder)?;
        let lvl2 = CacheLevel2::new(&folder)?;
        let map = HashMap::new();
        Ok(Self {folder, lvl2, map})
    }
    pub fn continued(folder: PathBuf) -> Result<Self, FolderCacheError> {
        let mut lvl2 = CacheLevel2::new(&folder)?;
        let mut map = HashMap::new();
        lvl2.load_to_hashmap(&folder, &mut map)?;
        Ok(Self {folder, lvl2, map})
    }
    pub fn insert<V: Serialize>(&mut self, k: K, v: &V) -> Result<(), FolderCacheError> {
        if let Some(old_ref) = self.map.get(&k) {
            let old_ref = old_ref.clone();
            if let Some((replace_k, new_ref)) = self.lvl2.overwrite(&self.folder, old_ref, &k, v)? {
                self.map.insert(k, new_ref);
                if let Some(moved_k) = replace_k {
                    self.map.insert(moved_k, old_ref);
                }
            }
        } else {
            let refv = self.lvl2.add(&self.folder, &k, v)?;
            self.map.insert(k, refv);
        }
        Ok(())
    }
    pub fn contains(&self, k: &K) -> bool {self.map.contains_key(k)}
    pub fn get<V: for <'a> Deserialize<'a>>(&mut self, k: &K) -> Result<V, FolderCacheError> {
        if let Some(refv) = self.map.get(k) {
            return Ok(self.lvl2.get_v(&self.folder, *refv)?);
        }
        return Err(FolderCacheError::Nothing);
    }
    pub fn remove(&mut self, k: &K) -> Result<(), FolderCacheError> {
        if let Some(old_ref) = self.map.remove(&k) {
            if let Some(other_k) = self.lvl2.remove(&self.folder, old_ref)? {
                self.map.insert(other_k, old_ref);
            }
        }
        Ok(())
    }
}

impl<K, V> CacheCompatible<K, V> for FolderCache<K> where
K: std::hash::Hash+Eq+Serialize+for <'a> Deserialize<'a>, V: Serialize+for <'a> Deserialize<'a> {
    type Error = FolderCacheError;

    fn contains(&self, k: K) -> bool { self.contains(&k) }
    fn get(&mut self, k: K) -> Result<V, Self::Error> { FolderCache::<K>::get(self, &k) }

    fn replace(&mut self, _: K, _: V) {}
}
impl<K, V> CacheMutCompatible<K, V> for FolderCache<K> where
K: std::hash::Hash+Eq+Serialize+for <'a> Deserialize<'a>, V: Serialize+for <'a> Deserialize<'a> {
    fn insert(&mut self, k: K, v: V) -> Result<(), Self::Error> { FolderCache::<K>::insert(self, k, &v) }

    fn remove(&mut self, k: K) -> Result<(), Self::Error> { FolderCache::<K>::remove(self, &k) }

    fn commit(&mut self) -> Result<(), Self::Error> { Ok(()) }
}
