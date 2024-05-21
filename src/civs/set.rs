use serde::{
    Serialize,Deserialize,
    ser::{Serializer,SerializeStruct},
    de::DeserializeOwned,
};
use byteorder::{LittleEndian,ReadBytesExt,WriteBytesExt};
use std::io::{Read,Write};

use crate::{
    Flags,Filled,Binary,
    civs::{Slot,TOMBS_LIMIT,AUTO_SHRINK_LIMIT},
};

#[derive(Deserialize)]
struct SerdeSetMultiSlot<K> {
    capacity: usize,
    data_size: usize,
    flags: Vec<u64>,
    data: Vec<K>,
}
impl<K> std::convert::TryFrom<SerdeSetMultiSlot<K>> for SetMultiSlot<K> {
    type Error = String;
    fn try_from(mut slot: SerdeSetMultiSlot<K>) -> Result<SetMultiSlot<K>,String> {
        if slot.data_size != std::mem::size_of::<K>() { return Err(format!("Unvalid data size {}, must be {}",std::mem::size_of::<K>(),slot.data_size)); }
        if (slot.data.len() > 0) && (slot.data.len() < slot.capacity) {
            slot.data.reserve(slot.capacity - slot.data.len());
        }
        Ok(SetMultiSlot {
            capacity: slot.capacity,
            flags: Flags(slot.flags),
            data: slot.data,
        })
    }
}

impl<K> Serialize for SetMultiSlot<K>
where
    K: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("SerdeSetMultiSlot", 4)?;
        state.serialize_field("capacity", &self.capacity)?;
        state.serialize_field("data_size", &std::mem::size_of::<K>())?;
        state.serialize_field("flags", &self.flags)?;
        state.serialize_field("data", &self.data)?;
        state.end()
    }
}

#[derive(Debug,Clone,Deserialize)]
#[serde(try_from = "SerdeSetMultiSlot<K>")]
pub(crate) struct SetMultiSlot<K> {
    capacity: usize,
    flags: Flags,
    data: Vec<K>,
}
impl<K> SetMultiSlot<K> {
    fn heap_mem(&self) -> usize {
        self.flags.heap_mem() + self.data.capacity() * std::mem::size_of::<K>()
    }
}
impl<K: Ord> SetMultiSlot<K> {
    fn new_empty(sz: usize, slot_sz: usize) -> SetMultiSlot<K> {
        let cap = slot_sz * (0x1 << (sz-1));
        SetMultiSlot {
            capacity: cap,
            flags: Flags::nulls(cap),
            data: Vec::with_capacity(cap),
        }
    }
    pub(crate) fn new(data: Vec<K>) -> SetMultiSlot<K> {
        SetMultiSlot {
            capacity: data.len(),
            flags: Flags::ones(data.len()),
            data: data,
        }
    }
    fn empty(&self) -> bool {
        self.data.len() == 0
    }
    fn check_len(&self) -> usize {
        self.flags.0.iter().fold(0,|acc,x| acc + x.count_ones() as usize)
    }
    fn contains(&self, k: &K) -> Option<usize> {
        if (self.data.len() == 0)||(*k < self.data[0])||(*k > self.data[self.data.len()-1]) { return None; }
        match self.data.binary_search(k) {
            Ok(idx) => match self.flags.get(idx) {
                true => Some(idx),
                false => None,
            },
            Err(_) => None,
        }
    }            
    fn clear(&mut self) {
        self.flags.set_nulls();
        self.data.clear();
    }
    fn shrink_to_fit(&mut self) {
        self.data.shrink_to_fit();
    }
    fn filtered_iter(&self) -> SetMultiSlotFilterIterator<K> {
        SetMultiSlotFilterIterator {
            iter: self.data.iter().enumerate(),
            flags: &self.flags,
        }
    }
}

struct SetMultiSlotFilterIterator<'t,K> {
    iter: std::iter::Enumerate<std::slice::Iter<'t,K>>,
    flags: &'t Flags,
}
impl<'t,K> Iterator for SetMultiSlotFilterIterator<'t,K> {
    type Item = &'t K;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.iter.next() {
                Some((n,k)) if self.flags.get(n) => break Some(k),
                Some(_) => continue,
                None => break None,
            }
        }
    }
}

pub struct Iter<'t,K> {
    slot_iter: Option<std::slice::Iter<'t,(K,())>>,
    cur_data_iter: Option<SetMultiSlotFilterIterator<'t,K>>,
    data_iter: Vec<SetMultiSlotFilterIterator<'t,K>>,
}
impl<'t,K> Iterator for Iter<'t,K> {
    type Item = &'t K;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(iter) = &mut self.slot_iter {
            match iter.next() {
                Some((k,())) => return Some(&k),
                None => { self.slot_iter = None; },
            }
        }
        while let Some(iter) = &mut self.cur_data_iter {
            match iter.next() {
                Some(k) => return Some(k),
                None => { self.cur_data_iter = self.data_iter.pop(); },
            }
        }
        None
    }
}


const CURRENT_CIVS_SET_VERSION: (u32,u32) = (0,1);

#[derive(Debug)]
pub enum CivSetIoError {
    WriteHeader,
    WriteSlot(bincode::Error),
    WriteData(bincode::Error),
    ReadHeader,
    ReadSlot(bincode::Error),
    ReadData(bincode::Error),
    InvalidHeader,
    InvalidVersion(u32,u32),
}

impl<K: Ord + Serialize + DeserializeOwned> Binary for CivSet<K> {
    type IoError = CivSetIoError;
    fn memory(&self) -> usize {
        let mut data_mem = self.data.capacity() * std::mem::size_of::<SetMultiSlot<K>>();
        for ms in &self.data {
            data_mem += ms.heap_mem();
        }
        std::mem::size_of::<CivSet<K>>() + self.slot.heap_mem() + data_mem
    }
    fn into_writer<W: Write>(&self, mut wrt: W) -> Result<(),Self::IoError> {
        let version = CURRENT_CIVS_SET_VERSION;
        write!(wrt,"CIVS").map_err(|_|CivSetIoError::WriteHeader)?;
        wrt.write_u32::<LittleEndian>(version.0).map_err(|_|CivSetIoError::WriteHeader)?;
        wrt.write_u32::<LittleEndian>(version.1).map_err(|_|CivSetIoError::WriteHeader)?;
        bincode::serialize_into(&mut wrt,&self.slot).map_err(CivSetIoError::WriteSlot)?;
        bincode::serialize_into(&mut wrt,&self.data).map_err(CivSetIoError::WriteData)
    }
    fn from_reader<R: Read>(mut rdr: R) -> Result<CivSet<K>,Self::IoError> {
        let mut buf = [0; 4];
        rdr.read_exact(&mut buf).map_err(|_|CivSetIoError::ReadHeader)?;
        if buf != "CIVS".as_bytes()[0..4] { return Err(CivSetIoError::InvalidHeader); }
        let maj = rdr.read_u32::<LittleEndian>().map_err(|_|CivSetIoError::ReadHeader)?;
        let min = rdr.read_u32::<LittleEndian>().map_err(|_|CivSetIoError::ReadHeader)?;
        if (maj != 0)||(min != 1) { return Err(CivSetIoError::InvalidVersion(maj,min)); }
        let slot: Slot<K,()> = bincode::deserialize_from(&mut rdr).map_err(CivSetIoError::ReadSlot)?;
        let data: Vec<SetMultiSlot<K>> = bincode::deserialize_from(&mut rdr).map_err(CivSetIoError::ReadData)?;
        let mut len = slot.len();
        let mut tombs = 0;
        for ms in &data {
            let ln = ms.check_len();
            len += ln;
            tombs += ms.capacity - ln;
        }
        Ok(CivSet {
            len: len,
            tombs: tombs,
            slot: slot,
            data: data,
            
            tmp_merge_vec: Vec::new(),
            tmp_merge_flags: Flags::tmp(),
        })
    }
}

#[derive(Clone)]
pub struct CivSet<K> {
    len: usize,
    tombs: usize,
    slot: Slot<K,()>,
    data: Vec<SetMultiSlot<K>>,

    tmp_merge_vec: Vec<K>,
    tmp_merge_flags: Flags,
}
impl<K: std::fmt::Debug> std::fmt::Debug for CivSet<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CivSet")
            .field("len", &self.len)
            .field("tombs", &self.tombs)
            .field("slot", &self.slot)
            .field("data", &self.data)
            .finish()
    }
}
impl<K: Ord> CivSet<K> {
    pub fn new() -> CivSet<K> {
        CivSet {
            len: 0,
            tombs: 0,
            slot: Slot::new(),
            data: Vec::new(),

            tmp_merge_vec: Vec::new(),
            tmp_merge_flags: Flags::tmp(),
        }
    }
    pub fn filtered_iter(&self) -> Iter<K> {
        let mut v = {
            let n = self.data.len();
            let mut v = Vec::with_capacity(n);            
            for i in 0 .. n {
                v.push(self.data[n-i-1].filtered_iter());
            }
            v
        };
        Iter {
            slot_iter: Some(self.slot.iter()),
            cur_data_iter: v.pop(),
            data_iter: v,            
        }
    }
    
    pub fn clear(&mut self) {
        self.len = 0;
        self.tombs = 0;
        self.slot.clear();
        self.data.clear();
        self.tmp_merge_flags = Flags::tmp();
        self.tmp_merge_vec.clear();
    }
    pub fn contains(&self, k: &K) -> bool {
        match self.slot.contains(k) {
            Some(_) => true,
            None => self.multy_contains(k).is_some(),
        }
    }    
    fn multy_contains(&self, k: &K) -> Option<(usize,usize)> {
        for (n,ms) in self.data.iter().enumerate() {
            if let Some(idx) = ms.contains(k) {
                return Some((n,idx));
            }
        }
        None
    }
    pub fn insert(&mut self, k: K) -> bool {
        // return true if value was inserted
        
        if self.multy_contains(&k).is_some() {
            return false;
        }
        let (r,filled) = self.slot.insert(k,());
        if let Filled::Full = filled {
            if self.data.len() == 0 {
                self.data.push(self.slot.into_set_multislot());
            } else {
                let mut n = 0;
                while (n < self.data.len())&&(!self.data[n].empty()) { n += 1; }
                if n == self.data.len() {
                    self.data.push(SetMultiSlot::new_empty(n+1,self.slot.max_size()));
                }
                if let Err(s) = self.merge_into(n) {
                    panic!("Unreachable merge_into: {}",s);
                }
                if let Err(s) = self.check_tombs(n) {
                    panic!("Unreachable check_tombs: {}",s);
                }
                self.shrink_long();
            }
        }
        match r {
            None => {
                self.len += 1;
                true
            },
            Some(_) => false,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn tombs(&self) -> usize {
        self.tombs
    }
    pub fn remove(&mut self, k: &K) -> bool {
        let r = match self.multy_contains(&k) {
            Some((msi,idx)) => {
                self.tombs += 1;
                self.data[msi].flags.unset(idx);
                true
            },
            None => self.slot.remove(k).is_some(),
        };
        if r { self.len -= 1; }
        r
    }
    pub fn shrink_to_fit(&mut self) {
        for ms in &mut self.data {
            ms.shrink_to_fit();
        }
    }
    fn shrink_long(&mut self) {
        for ms in &mut self.data {
            if (ms.capacity >= AUTO_SHRINK_LIMIT)&&(ms.empty()) {   
                ms.shrink_to_fit();
            }
        }
    }
    fn merge_into(&mut self, n: usize) -> Result<(),&'static str> {
        if !self.data[n].empty() { return Err("data[n] is not empty"); }
        let mut cnt = self.slot.len();
        for i in 0 .. n {
            if self.data[i].empty() { return Err("one of data[0..n] is empty"); }
            cnt += self.data[i].data.len();
        }
        self.data[n].data.reserve(cnt);
        
        {
            for p in self.slot.data.drain(..) {
                self.data[n].data.push(p.0);
            }
            self.slot.clear();

            std::mem::swap(&mut self.data[n].data, &mut self.tmp_merge_vec);
            for i in 0 .. n {
                std::mem::swap(&mut self.data[i].flags,&mut self.tmp_merge_flags);
                for (j,k) in self.data[i].data.drain(..).enumerate() {
                    if self.tmp_merge_flags.get(j) {
                        self.tmp_merge_vec.push(k);
                    }
                }
                std::mem::swap(&mut self.data[i].flags,&mut self.tmp_merge_flags);
                self.data[i].clear();
            }
            std::mem::swap(&mut self.data[n].data, &mut self.tmp_merge_vec);

            self.data[n].data.sort();
        }
        
        let c = self.data[n].data.len();
        self.data[n].flags.set_ones(c);
        Ok(())
    }
    fn check_tombs(&mut self, n: usize) -> Result<(),&'static str> {
        if self.data[n].empty() { return Err("data[n] is empty"); }
        for i in 0 .. n {
            if !self.data[i].empty() { return Err("one of data[0..n] is not empty"); }
        }

        let sz =  self.slot.max_size();
        let local_tombs = self.data[n].capacity - self.data[n].data.len();
        let local_part = (local_tombs as f64) / (self.data[n].capacity as f64);
        if (local_tombs > sz) && (local_part > TOMBS_LIMIT) {
            std::mem::swap(&mut self.data[n].data, &mut self.tmp_merge_vec);
            {
                let mut count = self.tmp_merge_vec.len();
                let mut iter = self.tmp_merge_vec.drain(..);

                let mut msi = self.data[..n].iter_mut();
                while let Some(ms) = msi.next_back() {
                    let cap = ms.capacity;
                    if count >= cap {
                        for _ in 0 .. cap {
                            if let Some(k) = iter.next() {
                                ms.data.push(k);
                            }
                        }
                        if ms.data.len() != cap {
                            return Err("data count < data.len()");
                        }
                        ms.flags.set_ones(cap);
                        count -= cap;
                        if count == 0 { break; }
                        continue;
                    }
                    if (cap - count) > sz { continue; }
                    // checked tombs = (cap - count) <= sz and local_tombs > sz
                    let d_tombs = local_tombs - (cap - count);
                    for _ in 0 .. count {
                        if let Some(k) = iter.next() {
                            ms.data.push(k);
                        }
                    }
                    if ms.data.len() != count {
                        return Err("data count < data.len()");
                    }
                    ms.flags.set_ones(count);
                    if d_tombs > self.tombs {
                        return Err("local_tombs > self.tombs");
                    }
                    self.tombs -= d_tombs;
                    break;
                }
                if let Some(_) = iter.next() {
                    return Err("merged data greater then the sum of the parts");
                }
            }
            std::mem::swap(&mut self.data[n].data, &mut self.tmp_merge_vec);
            self.data[n].clear();
        }
        Ok(())
    }
}

#[cfg(feature = "debug")]
impl<K: Ord> CivSet<K> {
    pub fn check_len(&self) -> usize {
        self.slot.len() + self.data.iter().fold(0,|acc,x|acc+x.check_len())
    }
    pub fn max_capacity(&self) -> usize {
        self.slot.max_size() + self.data.iter().fold(0,|acc,x|acc+x.capacity)
    }
    pub fn real_capacity(&self) -> usize {
        self.slot.max_size() + self.data.iter().fold(0,|acc,x|acc+x.data.capacity())
    }
    pub fn statistics(&self) -> Vec<String> {
        let mut s = (0,0,0);
        let mut v = Vec::new();
        for (i,ms) in self.data.iter().enumerate() {
            if !ms.empty() {
                let len = ms.check_len();
                let cap = ms.capacity;
                let tombs = cap - len;
                v.push(format!("{:3}: {:12} {:12} {:12}",i,cap,len,tombs));
                s.0 += cap;
                s.1 += len;
                s.2 += tombs;
            }
        }
        v.push(format!("TOT: {:12} {:12} {:12}",s.0,s.1,s.2));
        v
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn iterator() {
        let mut set = CivSet::new();
        let mut res = Vec::new();
        for i in 0 .. 1000 {
            set.insert(i);
            if i % 2 == 0 {
                res.push(i);
            }
        }
        for i in 0 .. 1000 {
            if i % 2 == 1 {
                set.remove(&i);
            }
        }

        let r = {
            let mut tmp = set.filtered_iter().map(|v|*v).collect::<Vec<_>>();
            tmp.sort();
            tmp
        };
        assert_eq!(res,r);
    }
}
