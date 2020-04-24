
use crate::{
    Flags,Filled,
    civs::{Slot,TOMBS_LIMIT},
};

pub enum RemovedItem<'t,V> {
    Ref(&'t mut V),
    Owned(V),
}
impl<'t,V> RemovedItem<'t,V> {
    pub fn swap(self, mut tmp: V) -> V {
        match self {
            RemovedItem::Ref(r) => {
                std::mem::swap(&mut tmp, r);
                tmp
            },
            RemovedItem::Owned(v) => v,
        }
    }
}
impl<'t,V> AsRef<V> for RemovedItem<'t,V> {
    fn as_ref(&self) -> &V {
        match self {
            RemovedItem::Ref(r) => r,
            RemovedItem::Owned(v) => v,
        }
    }
}
impl<'t,V> AsMut<V> for RemovedItem<'t,V> {
    fn as_mut(&mut self) -> &mut V {
        match self {
            RemovedItem::Ref(r) => r,
            RemovedItem::Owned(v) => v,
        }
    }
}
impl<'t,V: Copy> RemovedItem<'t,V> {
    pub fn copied(self) -> V {
        match self {
            RemovedItem::Ref(r) => *r,
            RemovedItem::Owned(v) => v,
        }
    }
}
impl<'t,V: Clone> RemovedItem<'t,V> {
    pub fn cloned(self) -> V {
        match self {
            RemovedItem::Ref(r) => r.clone(),
            RemovedItem::Owned(v) => v,
        }
    }
}

pub(crate) struct MapMultiSlot<K,V> {
    _sz: usize,
    empty: bool,
    flags: Flags,
    keys: Vec<K>,
    values: Vec<V>,
}
impl<K: Ord, V> MapMultiSlot<K,V> {
    pub(crate) fn new(data: Vec<(K,V)>) -> MapMultiSlot<K,V> {
        let len = data.len();
        let mut keys = Vec::with_capacity(len);
        let mut values = Vec::with_capacity(len);
        for (k,v) in data {
            keys.push(k);
            values.push(v);
        }
        MapMultiSlot {
            _sz: 1,
            empty: len == 0,
            flags: Flags::ones(len),
            keys: keys,
            values: values,
        }
    }
    fn empty(sz: usize, slot_sz: usize) -> MapMultiSlot<K,V> {
        MapMultiSlot {
            _sz: sz,
            empty: true,
            flags: Flags::nulls(slot_sz * (0x1 << (sz-1))),
            keys: Vec::new(),
            values: Vec::new(),
        }
    }
    fn contains(&self, k: &K) -> Option<usize> {
        if (self.keys.len() == 0)||(*k < self.keys[0])||(*k > self.keys[self.keys.len()-1]) { return None; }
        match self.keys.binary_search(k) {
            Ok(idx) => match self.flags.get(idx) {
                true => Some(idx),
                false => None,
            },
            Err(_) => None,
        }
    }            
    fn clear(&mut self) {
        self.empty = true;
        self.flags.set_nulls();
        self.keys.clear();
        self.values.clear();
    }
    fn shrink_to_fit(&mut self) {
        self.keys.shrink_to_fit();
        self.values.shrink_to_fit();
    }
    fn reserve(&mut self, cnt: usize) {
        self.keys.reserve(cnt);
        self.values.reserve(cnt);
    }
    fn drain(&mut self) -> MapMultiSlotDrainIterator<K,V> {
        MapMultiSlotDrainIterator {
            iter: self.keys.drain(..).zip(self.values.drain(..)),
        }
    }
    fn filtered_drain(&mut self) -> MapMultiSlotFilterDrainIterator<K,V> {
        MapMultiSlotFilterDrainIterator {
            iter: self.keys.drain(..).zip(self.values.drain(..)).enumerate(),
            flags: &self.flags,
        }
    }
    fn fill_in<'t>(&mut self, iter: &mut std::iter::Zip<std::vec::Drain<'t,K>,std::vec::Drain<'t,V>>) -> bool { // is exhausted
        let mut cur = 0;
        let cap = self.keys.capacity();
        while cur < cap {
            match iter.next() {
                Some((k,v)) => {
                    self.keys.push(k);
                    self.values.push(v);
                },
                None => return true,
            }
            cur += 1;
        }
        return false;
    }
}

struct MapMultiSlotFilterDrainIterator<'t,K,V> {
    iter: std::iter::Enumerate<std::iter::Zip<std::vec::Drain<'t,K>,std::vec::Drain<'t,V>>>,
    flags: &'t Flags,
}
impl<'t,K,V> Iterator for MapMultiSlotFilterDrainIterator<'t,K,V> {
    type Item = (K,V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.iter.next() {
                Some((n,(k,v))) if self.flags.get(n) => break Some((k,v)),
                Some(_) => continue,
                None => break None,
            }
        }
    }
}

struct MapMultiSlotDrainIterator<'t,K,V> {
    iter: std::iter::Zip<std::vec::Drain<'t,K>,std::vec::Drain<'t,V>>,
}
impl<'t,K,V> Iterator for MapMultiSlotDrainIterator<'t,K,V> {
    type Item = (K,V);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
      
pub struct CivMap<K,V> {
    len: usize,
    tombs: usize,
    slot: Slot<K,V>,
    data: Vec<MapMultiSlot<K,V>>,

    tmp_c: usize,
    tmp_merge_keys: Vec<K>,
    tmp_merge_values: Vec<V>,
}
       
impl<K: Ord, V> CivMap<K,V> {
    pub fn new() -> CivMap<K,V> {
        CivMap {
            len: 0,
            tombs: 0,
            slot: Slot::new(),
            data: Vec::new(),

            tmp_c: 0,
            tmp_merge_keys: Vec::new(),
            tmp_merge_values: Vec::new(),
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
    pub fn get(&self, k: &K) -> Option<&V> {
        match self.slot.get(k) {
            r @ Some(_) => r,
            None => match self.multy_contains(k) {
                Some((msi,idx)) => Some(&self.data[msi].values[idx]),
                None => None,
            }
        }
    }
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        match self.multy_contains(k) {
            Some((msi,idx)) => Some(&mut self.data[msi].values[idx]),
            None => self.slot.get_mut(k),
        }
    }
    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        if let Some((msi,idx)) = self.multy_contains(&k) {
            let mut tmp = v;
            std::mem::swap(&mut tmp, &mut self.data[msi].values[idx]);
            return Some(tmp);
        }
        let (r,filled) = self.slot.insert(k,v);
        if let Filled::Full = filled {
            if self.data.len() == 0 {
                self.data.push(self.slot.into_map_multislot());
            } else {
                let mut n = 0;
                while (n < self.data.len())&&(!self.data[n].empty) { n += 1; }
                if n == self.data.len() {
                    self.data.push(MapMultiSlot::empty(n+1,self.slot.max_size()));
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
        r
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn tombs(&self) -> usize {
        self.tombs
    }
    pub fn remove(&mut self, k: &K) -> Option<RemovedItem<V>> {
        match self.multy_contains(&k) {
            Some((msi,idx)) => {
                self.tombs += 1;
                self.data[msi].flags.unset(idx);
                Some(RemovedItem::Ref(&mut self.data[msi].values[idx]))
            },
            None => match self.slot.remove(k) {
                Some(v) => Some(RemovedItem::Owned(v)),
                None => None,
            },
        }
    }
    pub fn shrink_to_fit(&mut self) {
        for ms in &mut self.data {
            ms.shrink_to_fit();
        }
    }
    fn check_tombs(&mut self, n: usize) -> Result<(),&'static str> {
        if self.data[n].empty { return Err("data[n] is empty"); }
        for i in 0 .. n {
            if !self.data[i].empty { return Err("one of data[0..n] is not empty"); }
        }

        let sz =  self.slot.max_size();
        let local_tombs = self.data[n].keys.capacity() - self.data[n].keys.len();
        let local_part = (local_tombs as f64) / (self.data[n].keys.capacity() as f64);
        if (local_tombs > sz) && (local_part > TOMBS_LIMIT) {
            std::mem::swap(&mut self.data[n].keys, &mut self.tmp_merge_keys);
            std::mem::swap(&mut self.data[n].values, &mut self.tmp_merge_values);
            {
                let mut count = self.tmp_merge_keys.len();
                let mut iter = self.tmp_merge_keys.drain(..).zip(self.tmp_merge_values.drain(..));

                let mut msi = self.data[..n].iter_mut();
                while let Some(ms) = msi.next_back() {
                    let cap = ms.keys.capacity();
                    if count >= cap {
                        for _ in 0 .. cap {
                            if let Some((k,v)) = iter.next() {
                                ms.keys.push(k);
                                ms.values.push(v);
                            }
                        }
                        ms.empty = false;
                        if ms.keys.len() != cap {
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
                        if let Some((k,v)) = iter.next() {
                            ms.keys.push(k);
                            ms.values.push(v);
                        }
                    }
                    ms.empty = false;
                    if ms.keys.len() != count {
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
            std::mem::swap(&mut self.data[n].keys, &mut self.tmp_merge_keys);
            std::mem::swap(&mut self.data[n].values, &mut self.tmp_merge_values);
            self.data[n].clear();
        }
        Ok(())
    }
    fn merge_into(&mut self, n: usize) -> Result<(),&'static str> {
        // merge sort for sorted inflating vectors
        
        if !self.data[n].empty { return Err("data[n] is not empty"); }
        let mut cnt = self.slot.len();
        for i in 0 .. n {
            if self.data[i].empty { return Err("one of data[0..n] is empty"); }
            cnt += self.data[i].keys.len();
        }
        self.data[n].reserve(cnt);

        std::mem::swap(&mut self.data[n].keys, &mut self.tmp_merge_keys);
        std::mem::swap(&mut self.data[n].values, &mut self.tmp_merge_values);
        {
            if n == 0 {
                for (k,v) in self.slot.sorted_drain() {
                    self.tmp_merge_keys.push(k);
                    self.tmp_merge_values.push(v);
                }
                self.slot.clear();
            } else {
                let mut slot = self.slot.into_map_multislot();
                self.slot.clear();
                for i in 0 .. n {
                    { // for split_at_mut
                        let (sorted,to_sort) = self.data[..].split_at_mut(i);
                        
                        let mut f_data = slot.drain();
                        let mut s_data = to_sort[0].filtered_drain();
                        let mut sorted = sorted.iter_mut(); 
                        
                        let mut f = f_data.next();
                        let mut s = s_data.next();
                        
                        loop {
                            while f.is_some() && s.is_some() {
                                let fe = f.take().unwrap(); // safe
                                let se = s.take().unwrap(); // safe
                                match fe.0 < se.0 {
                                    true => {
                                        self.tmp_merge_keys.push(fe.0);
                                        self.tmp_merge_values.push(fe.1);
                                        f = f_data.next();
                                        s = Some(se);
                                    },
                                    false => {
                                        self.tmp_merge_keys.push(se.0);
                                        self.tmp_merge_values.push(se.1);                           
                                        f = Some(fe);
                                        s = s_data.next();
                                    },
                                }
                            }
                            if f.is_none() {
                                // f_data finished, try to get next
                                match sorted.next() {
                                    Some(ms) => {
                                        f_data = ms.drain();
                                        f = f_data.next();
                                    },
                                    None => break, // all fs are done
                                }
                            } else {
                                // s is done
                                break;
                            }
                        }
                        if f.is_some() {
                            loop {
                                while let Some(fe) = f {
                                    self.tmp_merge_keys.push(fe.0);
                                    self.tmp_merge_values.push(fe.1);
                                    f = f_data.next();
                                }
                                match sorted.next() {
                                    Some(ms) => {
                                        f_data = ms.drain();
                                        f = f_data.next();
                                    },
                                    None => break, // all fs are done
                                }
                            }
                        } else {
                            while let Some(se) = s {
                                self.tmp_merge_keys.push(se.0);
                                self.tmp_merge_values.push(se.1);
                                s = s_data.next();
                            }
                        }
                    }
                    
                    // fs and s are done, spliting tmp_merge_* into previous slots
                    //   on all iters except last
                    if i < (n-1) {
                        let mut iter = self.tmp_merge_keys.drain(..).zip(self.tmp_merge_values.drain(..));
                        let mut ex = slot.fill_in(&mut iter);
                        for j in 0 ..= i {
                            ex = self.data[j].fill_in(&mut iter);
                            if ex { break; }
                        }
                        if !ex {
                            if let Some(_) = iter.next() {
                                return Err("merged data greater then the sum of the parts");
                            }
                        }
                    }
                }
            }
            for i in 0 .. n {
                self.data[i].clear();
            }
            self.tmp_c += 1;
        }
        std::mem::swap(&mut self.data[n].keys, &mut self.tmp_merge_keys);
        std::mem::swap(&mut self.data[n].values, &mut self.tmp_merge_values);
   
        self.data[n].empty = false;
        let c = self.data[n].keys.len();
        self.data[n].flags.set_ones(c);
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_merge_sort_1() {
        let mut map: CivMap<u64,u32> = CivMap::new();
        map.slot = Slot::test(1);
        let test_data = vec![3,7,1,10,14,2,8,12,11,6,15,9,5,4,13].into_iter().map(|k|(k,k as u32)).collect::<Vec<_>>();
        for (k,v) in test_data {
            map.insert(k,v);
            println!("Size: {} ({})",map.len,map.tombs);
            println!("Slot: {:?}",map.slot);
            for (i,ms) in map.data.iter().enumerate() {
                println!("Data{:02}: {:?} -> {:?}",i,ms.keys,ms.values);
            }
            println!("");
        }
        panic!();
    }

    #[test]
    #[ignore]
    fn test_merge_sort_2() {
        let mut map: CivMap<u64,u32> = CivMap::new();
        map.slot = Slot::test(3);
        let test_data = vec![3,7,1,10,14,2,8,12,11,6,15,9,5,4,13].into_iter().map(|k|(k,k as u32)).collect::<Vec<_>>();
        for (k,v) in test_data {
            map.insert(k,v);
            println!("Size: {} ({})",map.len,map.tombs);
            println!("Slot: {:?}",map.slot);
            for (i,ms) in map.data.iter().enumerate() {
                println!("Data{:02}: {:?} -> {:?}",i,ms.keys,ms.values);
            }
            println!("");
        }
        for k in [4,8,5,11,7].iter() {
            map.remove(k);
        }
        
        map.insert(16,16);
        println!("Size: {} ({})",map.len,map.tombs);
        println!("Slot: {:?}",map.slot);
        for (i,ms) in map.data.iter().enumerate() {
            println!("Data{:02}: {:?} -> {:?}",i,ms.keys,ms.values);
        }
        println!("");
        panic!();
    }
    
}