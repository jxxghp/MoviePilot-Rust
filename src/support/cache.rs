use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// 提供带容量上限的最近最少使用缓存，避免用户配置产生无界全局状态。
pub(crate) struct BoundedCache<K, V> {
    capacity: usize,
    entries: HashMap<K, V>,
    usage: VecDeque<K>,
}

impl<K, V> BoundedCache<K, V>
where
    K: Clone + Eq + Hash,
{
    /// 创建指定容量的缓存，容量为零时不会保存任何条目。
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            usage: VecDeque::new(),
        }
    }

    /// 读取并提升条目的使用顺序。
    pub(crate) fn get_cloned(&mut self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    /// 插入或更新条目，并淘汰最久未使用的内容。
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.capacity == 0 {
            return;
        }
        self.entries.insert(key.clone(), value);
        self.touch(&key);
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.usage.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// 更新条目的最近使用位置。
    fn touch(&mut self, key: &K) {
        if let Some(index) = self.usage.iter().position(|item| item == key) {
            self.usage.remove(index);
        }
        self.usage.push_back(key.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedCache;

    /// 验证缓存达到容量后淘汰最久未使用的条目。
    #[test]
    fn evicts_least_recently_used_entry() {
        let mut cache = BoundedCache::new(2);
        cache.insert("first", 1);
        cache.insert("second", 2);
        assert_eq!(cache.get_cloned(&"first"), Some(1));

        cache.insert("third", 3);

        assert_eq!(cache.get_cloned(&"second"), None);
        assert_eq!(cache.get_cloned(&"first"), Some(1));
        assert_eq!(cache.get_cloned(&"third"), Some(3));
    }

    /// 验证重复插入会更新值和最近使用顺序。
    #[test]
    fn updates_existing_entry() {
        let mut cache = BoundedCache::new(1);
        cache.insert("key", 1);
        cache.insert("key", 2);

        assert_eq!(cache.get_cloned(&"key"), Some(2));
    }
}
