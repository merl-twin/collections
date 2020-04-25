use serde::{Serialize,Deserialize,ser::{Serializer,SerializeStruct}};

use crate::{
    Flags,Filled,
    civs::{Slot,TOMBS_LIMIT},
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
        if slot.data.len() < slot.capacity {
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
        let mut state = serializer.serialize_struct("SerdeMapMultiSlot", 4)?;
        state.serialize_field("capacity", &self.capacity)?;
        state.serialize_field("data_size", &std::mem::size_of::<K>())?;
        state.serialize_field("flags", &self.flags)?;
        state.serialize_field("data", &self.data)?;
        state.end()
    }
}

#[derive(Clone,Deserialize)]
#[serde(try_from = "SerdeSetMultiSlot<K>")]
pub(crate) struct SetMultiSlot<K> {
    capacity: usize,
    flags: Flags,
    data: Vec<K>,
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
}

#[derive(Clone,Serialize,Deserialize)]
pub struct CivSet<K> {
    len: usize,
    tombs: usize,
    slot: Slot<K,()>,
    data: Vec<SetMultiSlot<K>>,

    tmp_c: usize,
    tmp_merge_vec: Vec<K>,
    tmp_merge_flags: Flags,
}
       
impl<K: Ord> CivSet<K> {
    pub fn new() -> CivSet<K> {
        CivSet {
            len: 0,
            tombs: 0,
            slot: Slot::new(),
            data: Vec::new(),

            tmp_c: 0,
            tmp_merge_vec: Vec::new(),
            tmp_merge_flags: Flags::tmp(),
        }
    }
    pub fn contains(&mut self, k: &K) -> bool {
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
        if self.multy_contains(&k).is_some() {
            return true;
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
            }
        }
        self.len += 1;
        r.is_some()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn tombs(&self) -> usize {
        self.tombs
    }
    pub fn remove(&mut self, k: &K) -> bool {
        match self.multy_contains(&k) {
            Some((msi,idx)) => {
                self.tombs += 1;
                self.data[msi].flags.unset(idx);
                true
            },
            None => self.slot.remove(k).is_some(),
        }
    }
    pub fn shrink_to_fit(&mut self) {
        for ms in &mut self.data {
            ms.shrink_to_fit();
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
            self.tmp_c += 1;
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
        let local_tombs = self.data[n].data.capacity() - self.data[n].data.len();
        let local_part = (local_tombs as f64) / (self.data[n].data.capacity() as f64);
        if (local_tombs > sz) && (local_part > TOMBS_LIMIT) {
            std::mem::swap(&mut self.data[n].data, &mut self.tmp_merge_vec);
            {
                let mut count = self.tmp_merge_vec.len();
                let mut iter = self.tmp_merge_vec.drain(..);

                let mut msi = self.data[..n].iter_mut();
                while let Some(ms) = msi.next_back() {
                    let cap = ms.data.capacity();
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
