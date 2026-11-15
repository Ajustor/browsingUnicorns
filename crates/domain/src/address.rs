use std::fmt;
use std::str::FromStr;

use url::Url;

/// An absolute address the browser is allowed to load.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WebAddress(Url);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    Invalid(String),
    UnsupportedScheme(String),
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(raw) => write!(f, "invalid address: {raw}"),
            Self::UnsupportedScheme(scheme) => write!(f, "unsupported scheme: {scheme}"),
        }
    }
}

impl std::error::Error for AddressError {}

impl WebAddress {
    /// Schemes a page may be loaded from.
    pub const SCHEMES: &'static [&'static str] =
        &["http", "https", "file", "about", "chrome-extension"];

    pub fn parse(raw: &str) -> Result<Self, AddressError> {
        let url = Url::parse(raw.trim()).map_err(|_| AddressError::Invalid(raw.to_owned()))?;
        if !Self::SCHEMES.contains(&url.scheme()) {
            return Err(AddressError::UnsupportedScheme(url.scheme().to_owned()));
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }

    pub fn host(&self) -> Option<&str> {
        self.0.host_str()
    }

    pub fn is_secure(&self) -> bool {
        self.0.scheme() == "https"
    }
}

impl FromStr for WebAddress {
    type Err = AddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for WebAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_and_https() {
        let a = WebAddress::parse("https://example.com/a?b=c").unwrap();
        assert_eq!(a.host(), Some("example.com"));
        assert!(a.is_secure());
        assert!(!WebAddress::parse("http://example.com").unwrap().is_secure());
    }

    #[test]
    fn accepts_about_and_file() {
        assert!(WebAddress::parse("about:blank").is_ok());
        assert!(WebAddress::parse("file:///C:/index.html").is_ok());
    }

    #[test]
    fn rejects_other_schemes() {
        assert_eq!(
            WebAddress::parse("javascript:alert(1)"),
            Err(AddressError::UnsupportedScheme("javascript".into()))
        );
        assert!(matches!(
            WebAddress::parse("not a url"),
            Err(AddressError::Invalid(_))
        ));
    }
}
