use std::fmt;

/// Identifies a group of tabs for the whole life of the browser (never
/// reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u64);

impl GroupId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "group#{}", self.0)
    }
}

/// The colour of a group: its label and the line under its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupColor {
    Grey,
    Blue,
    Red,
    Yellow,
    Green,
    Pink,
    Purple,
    Cyan,
    Orange,
}

impl GroupColor {
    pub const ALL: [GroupColor; 9] = [
        Self::Grey,
        Self::Blue,
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Pink,
        Self::Purple,
        Self::Cyan,
        Self::Orange,
    ];

    /// The colour a new group gets: the first one no group has, in
    /// [`Self::ALL`]'s order but grey (else they cycle).
    pub fn next(used: impl IntoIterator<Item = GroupColor>) -> Self {
        let used: Vec<GroupColor> = used.into_iter().collect();
        let palette = &Self::ALL[1..];
        palette
            .iter()
            .copied()
            .find(|c| !used.contains(c))
            .unwrap_or(palette[used.len() % palette.len()])
    }

    /// Its name in the saved session.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grey => "grey",
            Self::Blue => "blue",
            Self::Red => "red",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Pink => "pink",
            Self::Purple => "purple",
            Self::Cyan => "cyan",
            Self::Orange => "orange",
        }
    }

    /// Back from [`Self::as_str`]; unknown names are grey.
    pub fn parse(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == name)
            .unwrap_or(Self::Grey)
    }
}

/// Tabs kept side by side under a name and a colour. Collapsed, only its
/// label shows in the tab strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    id: GroupId,
    pub name: String,
    pub color: GroupColor,
    pub collapsed: bool,
}

impl TabGroup {
    pub fn new(id: GroupId, color: GroupColor) -> Self {
        Self {
            id,
            name: String::new(),
            color,
            collapsed: false,
        }
    }

    pub fn id(&self) -> GroupId {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_groups_take_unused_colours_then_cycle() {
        assert_eq!(GroupColor::next([]), GroupColor::Blue);
        assert_eq!(GroupColor::next([GroupColor::Blue]), GroupColor::Red);
        assert_eq!(
            GroupColor::next([GroupColor::Red, GroupColor::Grey]),
            GroupColor::Blue
        );
        let all_but_grey = GroupColor::ALL[1..].to_vec();
        assert_eq!(GroupColor::next(all_but_grey), GroupColor::Blue);
    }

    #[test]
    fn colours_round_trip_by_name() {
        for color in GroupColor::ALL {
            assert_eq!(GroupColor::parse(color.as_str()), color);
        }
        assert_eq!(GroupColor::parse("mauve"), GroupColor::Grey);
    }
}
