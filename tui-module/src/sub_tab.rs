use std::fmt;

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SubTab {
    #[default]
    Albums,
    Artists,
    Playlists,
    Tracks,
}

impl SubTab {
    pub const fn selected(self) -> usize {
        match self {
            Self::Albums => 0,
            Self::Artists => 1,
            Self::Playlists => 2,
            Self::Tracks => 3,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Albums => Self::Artists,
            Self::Artists => Self::Playlists,
            Self::Playlists => Self::Tracks,
            Self::Tracks => Self::Albums,
        }
    }

    pub const fn previous(self) -> Self {
        match self {
            Self::Albums => Self::Tracks,
            Self::Artists => Self::Albums,
            Self::Playlists => Self::Artists,
            Self::Tracks => Self::Playlists,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
            Self::Tracks => "Tracks",
        }
    }

    pub const fn labels() -> [&'static str; 4] {
        ["Albums", "Artists", "Playlists", "Tracks"]
    }
}

impl fmt::Display for SubTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
