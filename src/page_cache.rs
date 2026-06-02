use crate::ReadablePage;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

const DEFAULT_CAPACITY: usize = 64;

pub struct PageCache {
    inner: Mutex<LruCache<(String, Option<usize>), ReadablePage>>,
}

impl PageCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            inner: Mutex::new(LruCache::new(cap)),
        }
    }

    pub fn get(&self, url: &str, max_chars: Option<usize>) -> Option<ReadablePage> {
        let mut cache = self.inner.lock().ok()?;
        cache.get(&(url.to_string(), max_chars)).cloned()
    }

    pub fn insert(&self, url: &str, max_chars: Option<usize>, page: ReadablePage) {
        if let Ok(mut cache) = self.inner.lock() {
            cache.put((url.to_string(), max_chars), page);
        }
    }
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExtractOptions;

    #[test]
    fn caches_by_url_and_max_chars() {
        let cache = PageCache::new(8);
        let page = ReadablePage {
            title: "T".into(),
            text: "body".into(),
            truncated: false,
        };
        cache.insert("https://example.com", None, page.clone());
        assert_eq!(cache.get("https://example.com", None).unwrap(), page);
        assert!(cache.get("https://example.com", Some(100)).is_none());
        assert!(cache.get("https://other.example", None).is_none());
    }

    #[test]
    fn evicts_least_recently_used() {
        let cache = PageCache::new(2);
        for i in 0..3 {
            cache.insert(
                &format!("https://example.com/{i}"),
                None,
                ReadablePage {
                    title: format!("{i}"),
                    text: String::new(),
                    truncated: false,
                },
            );
        }
        assert!(cache.get("https://example.com/0", None).is_none());
        assert!(cache.get("https://example.com/1", None).is_some());
        assert!(cache.get("https://example.com/2", None).is_some());
    }

    #[test]
    fn distinct_max_chars_keys() {
        let cache = PageCache::new(8);
        let full = ReadablePage {
            title: "T".into(),
            text: "long body".into(),
            truncated: false,
        };
        let short = ReadablePage {
            title: "T".into(),
            text: "long".into(),
            truncated: true,
        };
        cache.insert("https://example.com", None, full.clone());
        cache.insert(
            "https://example.com",
            ExtractOptions { max_chars: Some(4) }.max_chars,
            short.clone(),
        );
        assert_eq!(cache.get("https://example.com", None).unwrap(), full);
        assert_eq!(
            cache.get("https://example.com", Some(4)).unwrap(),
            short
        );
    }
}
