//! The sign-ins saved by the browser (user name and password by site), and
//! the sites' session cookies shared between the devices of a sync group.

use std::collections::{HashMap, HashSet};

use crate::Timestamp;

/// A user name and password saved for an origin (`https://host[:port]`).
/// `never`: the user asked never to save one for this origin (the user name
/// and password are then empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub origin: String,
    pub username: String,
    pub password: String,
    /// When it was saved or last changed: the most recent wins when devices
    /// are merged.
    pub updated_at: Timestamp,
    pub never: bool,
}

impl Credential {
    /// A "never" entry is told apart from a sign-in saved without user name.
    fn key(&self) -> (&str, &str, bool) {
        (&self.origin, &self.username, self.never)
    }
}

/// A credential that was deleted, kept so that merging with a device that
/// still has it does not bring it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRemoval {
    pub origin: String,
    pub username: String,
    pub at: Timestamp,
    /// The "never for this site" entry was deleted (not a sign-in).
    pub never: bool,
}

impl CredentialRemoval {
    fn key(&self) -> (&str, &str, bool) {
        (&self.origin, &self.username, self.never)
    }
}

/// Origin, user name and `never`: what tells credentials apart.
type Key = (String, String, bool);

fn owned((origin, username, never): (&str, &str, bool)) -> Key {
    (origin.to_owned(), username.to_owned(), never)
}

/// What saving a sign-in would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// Nothing to ask: already saved as is, or never for this site.
    Nothing,
    /// A new sign-in for the site.
    Save,
    /// A saved user name whose password changed.
    Update,
}

/// The saved sign-ins, one per origin and user name.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    items: Vec<Credential>,
    removed: Vec<CredentialRemoval>,
    /// Where each key is in `items` and in `removed`: merging a device's
    /// sign-ins looks each one up, not through the whole list.
    item_at: HashMap<Key, usize>,
    removal_at: HashMap<Key, usize>,
}

/// The indexes follow from the lists.
impl PartialEq for Credentials {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items && self.removed == other.removed
    }
}

impl Eq for Credentials {}

impl Credentials {
    /// Removals older than the newest ones beyond this count are forgotten.
    pub const MAX_REMOVALS: usize = 1_000;

    pub fn from_parts(items: Vec<Credential>, removed: Vec<CredentialRemoval>) -> Self {
        let mut all = Self::default();
        for c in items {
            all.apply(c);
        }
        for r in removed {
            all.apply_removal(r);
        }
        all
    }

    pub fn items(&self) -> &[Credential] {
        &self.items
    }

    pub fn removals(&self) -> &[CredentialRemoval] {
        &self.removed
    }

    /// The saved sign-ins (not the "never" entries), by origin then name.
    pub fn saved(&self) -> Vec<&Credential> {
        let mut saved: Vec<_> = self.items.iter().filter(|c| !c.never).collect();
        saved.sort_by(|a, b| a.key().cmp(&b.key()));
        saved
    }

    /// The sign-ins of `origin`, most recently changed first.
    pub fn for_origin(&self, origin: &str) -> Vec<&Credential> {
        let mut found: Vec<_> = self
            .items
            .iter()
            .filter(|c| !c.never && c.origin == origin)
            .collect();
        found.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
        found
    }

    /// Whether the user asked never to save sign-ins of `origin`.
    pub fn is_never(&self, origin: &str) -> bool {
        self.items.iter().any(|c| c.never && c.origin == origin)
    }

    /// What saving `username` and `password` for `origin` would do.
    pub fn offer(&self, origin: &str, username: &str, password: &str) -> Offer {
        if password.is_empty() || self.is_never(origin) {
            return Offer::Nothing;
        }
        match self
            .items
            .iter()
            .find(|c| !c.never && c.origin == origin && c.username == username)
        {
            Some(c) if c.password == password => Offer::Nothing,
            Some(_) => Offer::Update,
            None => Offer::Save,
        }
    }

    /// Saves (or updates) a sign-in.
    pub fn save(&mut self, origin: &str, username: &str, password: &str, at: Timestamp) {
        self.apply(Credential {
            origin: origin.to_owned(),
            username: username.trim().to_owned(),
            password: password.to_owned(),
            updated_at: at,
            never: false,
        });
    }

    /// Never offers to save sign-ins of `origin` again.
    pub fn never_for(&mut self, origin: &str, at: Timestamp) {
        self.apply(Credential {
            origin: origin.to_owned(),
            username: String::new(),
            password: String::new(),
            updated_at: at,
            never: true,
        });
    }

    /// Deletes the sign-in of `username` on `origin` (with `never`, the
    /// "never for this site" entry of `origin`, whose name is empty).
    /// Returns whether it was there.
    pub fn remove(&mut self, origin: &str, username: &str, never: bool, at: Timestamp) -> bool {
        let before = self.items.len();
        self.apply_removal(CredentialRemoval {
            origin: origin.to_owned(),
            username: if never { "" } else { username }.to_owned(),
            at,
            never,
        });
        self.items.len() != before
    }

    /// Takes in the other device's sign-ins and removals: the most recent
    /// change of each wins. Returns whether something changed.
    pub fn merge(&mut self, other: &Credentials) -> bool {
        let mut changed = false;
        for c in &other.items {
            changed |= self.apply(c.clone());
        }
        for r in &other.removed {
            changed |= self.apply_removal(r.clone());
        }
        changed
    }

    /// Returns whether it changed something.
    fn apply(&mut self, c: Credential) -> bool {
        let key = owned(c.key());
        let removal = self.removal_at.get(&key).copied();
        if removal.is_some_and(|i| self.removed[i].at >= c.updated_at) {
            return false;
        }
        match self.item_at.get(&key) {
            Some(&i) if self.items[i].updated_at >= c.updated_at => return false,
            Some(&i) => self.items[i] = c,
            None => {
                // Saved again since it was removed.
                if let Some(i) = removal {
                    self.removed.remove(i);
                    self.removal_at = index(&self.removed, CredentialRemoval::key);
                }
                self.item_at.insert(key, self.items.len());
                self.items.push(c);
            }
        }
        true
    }

    /// Returns whether it changed something.
    fn apply_removal(&mut self, r: CredentialRemoval) -> bool {
        let key = owned(r.key());
        let mut changed = false;
        if let Some(&i) = self.item_at.get(&key)
            && self.items[i].updated_at <= r.at
        {
            self.items.remove(i);
            self.item_at = index(&self.items, Credential::key);
            changed = true;
        }
        match self.removal_at.get(&key) {
            Some(&i) if self.removed[i].at >= r.at => {}
            Some(&i) => {
                self.removed[i].at = r.at;
                changed = true;
            }
            None => {
                self.removal_at.insert(key, self.removed.len());
                self.removed.push(r);
                changed = true;
            }
        }
        if self.removed.len() > Self::MAX_REMOVALS {
            self.removed.sort_by_key(|r| std::cmp::Reverse(r.at));
            self.removed.truncate(Self::MAX_REMOVALS);
            self.removal_at = index(&self.removed, CredentialRemoval::key);
        }
        changed
    }
}

/// Where each key of `list` is.
fn index<T>(list: &[T], key: impl Fn(&T) -> (&str, &str, bool)) -> HashMap<Key, usize> {
    list.iter()
        .enumerate()
        .map(|(i, x)| (owned(key(x)), i))
        .collect()
}

/// A cookie as the web views store it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteCookie {
    /// The host it was set for; with a leading dot, its subdomains too.
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// `Strict`, `Lax` or `None` (empty: unspecified).
    pub same_site: String,
    /// When it expires (seconds since the epoch); `None`: at the end of the
    /// browsing session.
    pub expires: Option<i64>,
}

impl SiteCookie {
    /// Cookies larger than this are not shared.
    pub const MAX_VALUE: usize = 4_096;

    /// The host without its leading dot, lowercase.
    pub fn host(&self) -> String {
        self.domain.trim_start_matches('.').to_ascii_lowercase()
    }

    pub fn is_expired(&self, now: Timestamp) -> bool {
        self.expires.is_some_and(|e| e <= now as i64)
    }

    /// What tells a cookie from another one (and its value apart):
    /// imported or exported once, it is never imported again. A hash (128
    /// bits), so that the value is not kept as is.
    pub fn fingerprint(&self) -> String {
        let text = format!(
            "{}\u{1}{}\u{1}{}\u{1}{}",
            self.domain.to_ascii_lowercase(),
            self.path,
            self.name,
            self.value
        );
        // FNV-1a, twice with different bases.
        let fnv = |basis: u64| {
            text.bytes().fold(basis, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        };
        format!(
            "{:016x}{:016x}",
            fnv(0xcbf2_9ce4_8422_2325),
            fnv(0x6c62_272e_07bb_0142)
        )
    }

    fn slot(&self) -> (String, &str, &str) {
        (self.domain.to_ascii_lowercase(), &self.path, &self.name)
    }

    /// Its size in a sync record, roughly.
    fn weight(&self) -> usize {
        self.domain.len() + self.name.len() + self.value.len() + self.path.len() + 64
    }
}

/// Fingerprints ([`SiteCookie::fingerprint`]) of the cookies this device
/// exported or imported, the oldest forgotten beyond [`Self::CAPACITY`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenCookies {
    order: Vec<String>,
    set: HashSet<String>,
}

impl SeenCookies {
    pub const CAPACITY: usize = 20_000;

    pub fn from_fingerprints(fingerprints: Vec<String>) -> Self {
        let mut seen = Self::default();
        seen.add(fingerprints);
        seen
    }

    /// Oldest first.
    pub fn fingerprints(&self) -> &[String] {
        &self.order
    }

    pub fn set(&self) -> &HashSet<String> {
        &self.set
    }

    /// Remembers `fingerprints`; returns whether one was new.
    pub fn add(&mut self, fingerprints: impl IntoIterator<Item = String>) -> bool {
        let mut added = false;
        for f in fingerprints {
            if self.set.insert(f.clone()) {
                self.order.push(f);
                added = true;
            }
        }
        let excess = self.order.len().saturating_sub(Self::CAPACITY);
        for f in self.order.drain(..excess) {
            self.set.remove(&f);
        }
        added
    }
}

/// The cookies this device sends to the others: unexpired, not too big,
/// those of the most recently visited sites first, within `budget` bytes.
/// `recent_hosts`: hosts visited, most recent first.
pub fn cookies_to_share(
    cookies: Vec<SiteCookie>,
    recent_hosts: &[String],
    now: Timestamp,
    budget: usize,
) -> Vec<SiteCookie> {
    // The best rank of each host, and of each domain over the hosts under
    // it (a cookie of `.example.com` serves `www.example.com`).
    let mut exact: HashMap<&str, usize> = HashMap::new();
    let mut under: HashMap<&str, usize> = HashMap::new();
    for (i, host) in recent_hosts.iter().enumerate() {
        exact.entry(host).or_insert(i);
        let mut rest = host.as_str();
        loop {
            under.entry(rest).or_insert(i);
            match rest.find('.') {
                Some(dot) => rest = &rest[dot + 1..],
                None => break,
            }
        }
    }
    let rank_of = |c: &SiteCookie| {
        let ranks = if c.domain.starts_with('.') {
            &under
        } else {
            &exact
        };
        ranks.get(c.host().as_str()).copied().unwrap_or(usize::MAX)
    };
    let mut cookies: Vec<_> = cookies
        .into_iter()
        .filter(|c| !c.is_expired(now) && c.value.len() <= SiteCookie::MAX_VALUE)
        .map(|c| (rank_of(&c), c))
        .collect();
    cookies.sort_by_key(|(rank, _)| *rank);
    let mut used = 0;
    cookies
        .into_iter()
        .filter(|(rank, _)| *rank != usize::MAX)
        .filter_map(|(_, c)| {
            used += c.weight();
            (used <= budget).then_some(c)
        })
        .collect()
}

/// The other devices' cookies to set here: unexpired, never seen here
/// before (`seen`: fingerprints of the cookies imported or exported), and
/// not replacing one this device has. `remote`: their cookies, most recent
/// device first (its cookie wins over another's in the same slot).
pub fn cookies_to_import<'a>(
    local: &[SiteCookie],
    remote: impl IntoIterator<Item = &'a SiteCookie>,
    seen: &HashSet<String>,
    now: Timestamp,
) -> Vec<SiteCookie> {
    let mut taken: HashSet<_> = local.iter().map(SiteCookie::slot).collect();
    let mut out = Vec::new();
    for c in remote {
        if c.is_expired(now) || seen.contains(&c.fingerprint()) || !taken.insert(c.slot()) {
            continue;
        }
        out.push(c.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://a.com";

    #[test]
    fn offers_follow_what_is_saved() {
        let mut c = Credentials::default();
        assert_eq!(c.offer(SITE, "me", "pw"), Offer::Save);
        assert_eq!(c.offer(SITE, "me", ""), Offer::Nothing);
        c.save(SITE, " me ", "pw", 1);
        assert_eq!(c.offer(SITE, "me", "pw"), Offer::Nothing);
        assert_eq!(c.offer(SITE, "me", "new"), Offer::Update);
        assert_eq!(c.offer(SITE, "you", "pw"), Offer::Save);
        c.never_for("https://b.com", 2);
        assert_eq!(c.offer("https://b.com", "me", "pw"), Offer::Nothing);
        assert_eq!(c.saved().len(), 1);
        assert!(c.for_origin("https://b.com").is_empty());
    }

    #[test]
    fn updates_replace_and_the_newest_comes_first() {
        let mut c = Credentials::default();
        c.save(SITE, "me", "old", 1);
        c.save(SITE, "me", "new", 2);
        c.save(SITE, "you", "pw", 3);
        let found = c.for_origin(SITE);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].username, "you");
        assert_eq!(found[1].password, "new");
    }

    #[test]
    fn merging_keeps_the_latest_change_and_removals() {
        let mut desktop = Credentials::default();
        desktop.save(SITE, "me", "pw1", 1);
        desktop.save("https://b.com", "me", "b", 1);
        let mut phone = desktop.clone();
        phone.save(SITE, "me", "pw2", 5);
        phone.remove("https://b.com", "me", false, 6);

        assert!(desktop.merge(&phone));
        assert_eq!(desktop.for_origin(SITE)[0].password, "pw2");
        assert!(desktop.for_origin("https://b.com").is_empty());
        assert!(!desktop.merge(&phone), "nothing new");

        // A removal older than a new save does not delete it.
        let mut later = Credentials::default();
        later.save("https://b.com", "me", "again", 9);
        assert!(desktop.merge(&later));
        assert_eq!(desktop.for_origin("https://b.com").len(), 1);
    }

    #[test]
    fn removing_a_never_entry_offers_again() {
        let mut c = Credentials::default();
        c.never_for(SITE, 1);
        assert!(c.remove(SITE, "", true, 2));
        assert_eq!(c.offer(SITE, "me", "pw"), Offer::Save);
    }

    #[test]
    fn a_never_entry_and_a_sign_in_without_name_are_apart() {
        let mut c = Credentials::default();
        c.save(SITE, "", "pw", 1);
        c.never_for(SITE, 2);
        assert_eq!(c.items().len(), 2, "never keeps the saved password");
        assert_eq!(c.for_origin(SITE)[0].password, "pw");

        let mut synced = c.clone();
        assert!(synced.remove(SITE, "", true, 3));
        assert_eq!(synced.items().len(), 1, "only the never entry goes");
        assert!(!synced.is_never(SITE));
        assert!(c.merge(&synced));
        assert_eq!(c.for_origin(SITE).len(), 1);
        assert!(!c.is_never(SITE));

        assert!(c.remove(SITE, "", false, 4));
        assert!(c.items().is_empty());
    }

    fn cookie(domain: &str, name: &str, value: &str) -> SiteCookie {
        SiteCookie {
            domain: domain.into(),
            name: name.into(),
            value: value.into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: String::new(),
            expires: Some(1_000),
        }
    }

    #[test]
    fn shared_cookies_are_those_of_visited_sites_within_budget() {
        let mut expired = cookie("a.com", "old", "1");
        expired.expires = Some(10);
        let cookies = vec![
            cookie("tracker.net", "id", "1"),
            cookie(".b.com", "sid", "2"),
            cookie("a.com", "sid", "3"),
            expired,
        ];
        let hosts = vec!["a.com".to_owned(), "www.b.com".to_owned()];
        let shared = cookies_to_share(cookies.clone(), &hosts, 100, 10_000);
        let names: Vec<_> = shared.iter().map(|c| c.domain.as_str()).collect();
        assert_eq!(names, ["a.com", ".b.com"]);
        let tight = cookies_to_share(cookies, &hosts, 100, 80);
        assert_eq!(tight.len(), 1);
        assert_eq!(tight[0].domain, "a.com");
        // A host cookie serves its host only.
        let www = cookies_to_share(vec![cookie("b.com", "sid", "1")], &hosts, 100, 10_000);
        assert!(www.is_empty());
    }

    #[test]
    fn merging_reports_only_real_changes() {
        let mut a = Credentials::default();
        a.save(SITE, "me", "pw", 1);
        a.never_for("https://n.com", 1);
        a.remove("https://b.com", "x", false, 2);
        let b = a.clone();
        assert!(!a.merge(&b));
        let mut newer = b.clone();
        newer.save(SITE, "me", "pw2", 3);
        assert!(a.merge(&newer));
        assert!(!a.merge(&newer));
        assert_eq!(a, newer);
        // Re-saved after a removal: the removal goes.
        let mut back = Credentials::default();
        back.save("https://b.com", "x", "pw", 5);
        assert!(a.merge(&back));
        assert!(a.removals().is_empty());
        assert_eq!(a.for_origin("https://b.com").len(), 1);
        // Removed again, later: it goes again.
        let mut gone = Credentials::default();
        gone.remove("https://b.com", "x", false, 6);
        assert!(a.merge(&gone));
        assert!(a.for_origin("https://b.com").is_empty());
        assert_eq!(a.for_origin(SITE)[0].password, "pw2", "index kept right");
    }

    #[test]
    fn remote_cookies_are_imported_once_and_never_over_local_ones() {
        let local = vec![cookie("a.com", "sid", "mine")];
        let remote = [
            cookie("a.com", "sid", "theirs"),
            cookie("b.com", "sid", "b1"),
            cookie("b.com", "sid", "b2"),
            cookie("c.com", "sid", "c"),
        ];
        let mut seen = HashSet::new();
        seen.insert(remote[3].fingerprint());
        let import = cookies_to_import(&local, &remote, &seen, 100);
        assert_eq!(import, vec![cookie("b.com", "sid", "b1")]);
    }
}
