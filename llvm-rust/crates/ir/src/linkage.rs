//! Linkage and visibility.

/// LLVM linkage kinds supported by the first slice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Linkage {
    #[default]
    External,
    Private,
    Internal,
    AvailableExternally,
    LinkOnce,
    Weak,
    Common,
    Appending,
    ExternWeak,
    LinkOnceOdr,
    WeakOdr,
}

impl Linkage {
    /// Returns the keyword, or the empty string for external linkage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::External => "",
            Self::Private => "private",
            Self::Internal => "internal",
            Self::AvailableExternally => "available_externally",
            Self::LinkOnce => "linkonce",
            Self::Weak => "weak",
            Self::Common => "common",
            Self::Appending => "appending",
            Self::ExternWeak => "extern_weak",
            Self::LinkOnceOdr => "linkonce_odr",
            Self::WeakOdr => "weak_odr",
        }
    }

    /// Parses a linkage keyword.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "" | "external" => Self::External,
            "private" => Self::Private,
            "internal" => Self::Internal,
            "available_externally" => Self::AvailableExternally,
            "linkonce" => Self::LinkOnce,
            "weak" => Self::Weak,
            "common" => Self::Common,
            "appending" => Self::Appending,
            "extern_weak" => Self::ExternWeak,
            "linkonce_odr" => Self::LinkOnceOdr,
            "weak_odr" => Self::WeakOdr,
            _ => return None,
        })
    }
}

/// Symbol visibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Visibility {
    #[default]
    Default,
    Hidden,
    Protected,
}

impl Visibility {
    /// Returns the keyword, or the empty string for default visibility.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::Hidden => "hidden",
            Self::Protected => "protected",
        }
    }

    /// Hidden and protected symbols are dso_local even without the keyword.
    pub const fn implies_dso_local(self) -> bool {
        matches!(self, Self::Hidden | Self::Protected)
    }

    /// Parses a visibility keyword.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "default" => Self::Default,
            "hidden" => Self::Hidden,
            "protected" => Self::Protected,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_keywords() {
        for linkage in [
            Linkage::External,
            Linkage::Private,
            Linkage::Internal,
            Linkage::WeakOdr,
        ] {
            assert_eq!(Linkage::parse(linkage.as_str()), Some(linkage));
        }
        assert_eq!(Visibility::parse("hidden"), Some(Visibility::Hidden));
        assert_eq!(Visibility::Hidden.as_str(), "hidden");
    }
}
