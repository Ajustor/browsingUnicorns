use crate::WebAddress;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub address: WebAddress,
    pub title: String,
}

impl Bookmark {
    /// The title, else the host, else the address.
    pub fn label(&self) -> &str {
        let title = self.title.trim();
        if !title.is_empty() {
            return title;
        }
        self.address.host().unwrap_or(self.address.as_str())
    }
}

/// Saved pages, in the order they were added; one per address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks {
    items: Vec<Bookmark>,
}

impl Bookmarks {
    pub fn from_items(items: Vec<Bookmark>) -> Self {
        let mut bookmarks = Self::default();
        for b in items {
            bookmarks.add(b.address, b.title);
        }
        bookmarks
    }

    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, address: &WebAddress) -> bool {
        self.items.iter().any(|b| &b.address == address)
    }

    /// Adds a bookmark; returns `false` when the address is already saved.
    pub fn add(&mut self, address: WebAddress, title: impl Into<String>) -> bool {
        if self.contains(&address) {
            return false;
        }
        self.items.push(Bookmark {
            address,
            title: title.into().trim().to_owned(),
        });
        true
    }

    pub fn remove(&mut self, address: &WebAddress) -> bool {
        let before = self.items.len();
        self.items.retain(|b| &b.address != address);
        self.items.len() != before
    }

    /// Adds the page when it is not saved, removes it otherwise. Returns
    /// whether it is saved afterwards.
    pub fn toggle(&mut self, address: WebAddress, title: impl Into<String>) -> bool {
        if self.remove(&address) {
            false
        } else {
            self.add(address, title)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn one_bookmark_per_address() {
        let mut b = Bookmarks::default();
        assert!(b.add(addr("https://a.com/"), "A"));
        assert!(!b.add(addr("https://a.com/"), "again"));
        assert_eq!(b.len(), 1);
        let loaded = Bookmarks::from_items(vec![b.items()[0].clone(), b.items()[0].clone()]);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn toggle_adds_then_removes() {
        let mut b = Bookmarks::default();
        assert!(b.toggle(addr("https://a.com/"), "A"));
        assert!(b.contains(&addr("https://a.com/")));
        assert!(!b.toggle(addr("https://a.com/"), "A"));
        assert!(b.is_empty());
        assert!(!b.remove(&addr("https://a.com/")));
    }

    #[test]
    fn label_falls_back_to_host() {
        let mut b = Bookmarks::default();
        b.add(addr("https://a.com/x"), "  ");
        b.add(addr("file:///C:/x.html"), "");
        assert_eq!(b.items()[0].label(), "a.com");
        assert_eq!(b.items()[1].label(), "file:///C:/x.html");
    }
}
