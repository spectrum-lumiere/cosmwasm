// this module requires iterator to be useful at all
#![cfg(feature = "iterator")]

use cosmwasm_std::{to_vec, Order, StdError, StdResult, Storage, KV};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::namespace_helpers::{
    get_with_prefix, range_with_prefix, remove_with_prefix, set_with_prefix,
};
use crate::type_helpers::{deserialize_kv, may_deserialize, must_deserialize};
use crate::{to_length_prefixed, to_length_prefixed_nested};

/// IndexedBucket works like a bucket but has a secondary index
/// This is a WIP.
/// Step 1 - allow exactly 1 secondary index, no multi-prefix on primary key
/// Step 2 - allow multiple named secondary indexes, no multi-prefix on primary key
/// Step 3 - allow multiple named secondary indexes, clean composite key support
///
/// Current Status: 0
pub struct IndexedBucket<'a, S, T>
where
    S: Storage,
    T: Serialize + DeserializeOwned,
{
    storage: &'a mut S,
    prefix_pk: Vec<u8>,
    prefix_idx: Vec<u8>,
    indexer: fn(&T) -> Vec<u8>,
}

impl<'a, S, T> IndexedBucket<'a, S, T>
where
    S: Storage,
    T: Serialize + DeserializeOwned,
{
    pub fn new(storage: &'a mut S, namespace: &[u8], indexer: fn(&T) -> Vec<u8>) -> Self {
        IndexedBucket {
            storage,
            prefix_pk: to_length_prefixed_nested(&[namespace, b"pk"]),
            prefix_idx: to_length_prefixed_nested(&[namespace, b"idx"]),
            indexer,
        }
    }

    /// save will serialize the model and store, returns an error on serialization issues.
    /// this must load the old value to update the indexes properly
    /// if you loaded the old value earlier in the same function, use replace to avoid needless db reads
    pub fn save(&mut self, key: &[u8], data: &T) -> StdResult<()> {
        let old_data = self.may_load(key)?;
        self.replace(key, Some(data), old_data.as_ref())
    }

    pub fn remove(&mut self, key: &[u8]) -> StdResult<()> {
        let old_data = self.may_load(key)?;
        self.replace(key, None, old_data.as_ref())
    }

    /// replace writes data to key. old_data must be the current stored value (from a previous load)
    /// and is used to properly update the index. This is used by save, replace, and update
    /// and can be called directly if you want to optimize
    pub fn replace(&mut self, key: &[u8], data: Option<&T>, old_data: Option<&T>) -> StdResult<()> {
        if let Some(old) = old_data {
            let old_idx = (self.indexer)(old);
            self.remove_from_index(&old_idx, key);
        }
        if let Some(updated) = data {
            let new_idx = (self.indexer)(updated);
            self.add_to_index(&new_idx, key);
            set_with_prefix(self.storage, &self.prefix_pk, key, &to_vec(updated)?);
        } else {
            remove_with_prefix(self.storage, &self.prefix_pk, key);
        }
        Ok(())
    }

    // index is stored (namespace, idx): key -> b"1"
    // idx is prefixed and appended to namespace
    pub fn add_to_index(&mut self, idx: &[u8], key: &[u8]) {
        // TODO: make this a bit cleaner
        let mut index_space = self.prefix_idx.clone();
        let mut key_prefix = to_length_prefixed(idx);
        index_space.append(&mut key_prefix);
        set_with_prefix(self.storage, &self.index_space(idx), key, b"1");
    }

    // index is stored (namespace, idx): key -> b"1"
    // idx is prefixed and appended to namespace
    pub fn remove_from_index(&mut self, idx: &[u8], key: &[u8]) {
        remove_with_prefix(self.storage, &self.index_space(idx), key);
    }

    // TODO: make this a bit cleaner
    fn index_space(&self, idx: &[u8]) -> Vec<u8> {
        let mut index_space = self.prefix_idx.clone();
        let mut key_prefix = to_length_prefixed(idx);
        index_space.append(&mut key_prefix);
        index_space
    }

    /// load will return an error if no data is set at the given key, or on parse error
    pub fn load(&self, key: &[u8]) -> StdResult<T> {
        let value = get_with_prefix(self.storage, &self.prefix_pk, key);
        must_deserialize(&value)
    }

    /// may_load will parse the data stored at the key if present, returns Ok(None) if no data there.
    /// returns an error on issues parsing
    pub fn may_load(&self, key: &[u8]) -> StdResult<Option<T>> {
        let value = get_with_prefix(self.storage, &self.prefix_pk, key);
        may_deserialize(&value)
    }

    /// iterates over the items in pk order
    pub fn range<'b>(
        &'b self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        order: Order,
    ) -> Box<dyn Iterator<Item = StdResult<KV<T>>> + 'b> {
        let mapped = range_with_prefix(self.storage, &self.prefix_pk, start, end, order)
            .map(deserialize_kv::<T>);
        Box::new(mapped)
    }

    /// returns all pks that where stored under this secondary index, always Ascending
    /// this is mainly an internal function, but can be used direcly if you just want to list ids cheaply
    pub fn pks_by_index<'b>(&'b self, idx: &[u8]) -> Box<dyn Iterator<Item = Vec<u8>> + 'b> {
        let start = self.index_space(idx);
        // end is the next byte
        let mut end = start.clone();
        let l = end.len();
        end[l - 1] += 1;
        let mapped = range_with_prefix(
            self.storage,
            &self.prefix_idx,
            Some(&start),
            Some(&end),
            Order::Ascending,
        )
        .map(|(k, _)| k);
        Box::new(mapped)
    }

    /// returns all items that match this secondary index, always by pk Ascending
    pub fn items_by_index<'b>(
        &'b self,
        idx: &[u8],
    ) -> Box<dyn Iterator<Item = StdResult<KV<T>>> + 'b> {
        let mapped = self.pks_by_index(idx).map(move |pk| {
            let v = self.load(&pk)?;
            Ok((pk, v))
        });
        Box::new(mapped)
    }

    /// Loads the data, perform the specified action, and store the result
    /// in the database. This is shorthand for some common sequences, which may be useful.
    ///
    /// If the data exists, `action(Some(value))` is called. Otherwise `action(None)` is called.
    pub fn update<A, E>(&mut self, key: &[u8], action: A) -> Result<T, E>
    where
        A: FnOnce(Option<T>) -> Result<T, E>,
        E: From<StdError>,
    {
        // we cannot copy index and it is consumed by the action, so we cannot use input inside replace
        // thus, we manually take care of removing the old index on success
        let input = self.may_load(key)?;
        let old_idx = input.as_ref().map(self.indexer);

        let output = action(input)?;

        // manually remove the old index if needed
        if let Some(idx) = old_idx {
            self.remove_from_index(&idx, key);
        }
        self.replace(key, Some(&output), None)?;
        Ok(output)
    }
}
