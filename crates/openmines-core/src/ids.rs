use std::fmt;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct PlayerId(pub i32);

impl From<i32> for PlayerId {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<PlayerId> for i32 {
    fn from(value: PlayerId) -> Self {
        value.0
    }
}

impl std::borrow::Borrow<i32> for PlayerId {
    fn borrow(&self) -> &i32 {
        &self.0
    }
}

impl PlayerId {
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self.0
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for PlayerId {
    type Err = <i32 as std::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl std::ops::Neg for PlayerId {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl PartialEq<i32> for PlayerId {
    fn eq(&self, other: &i32) -> bool {
        self.0 == *other
    }
}

impl PartialEq<PlayerId> for i32 {
    fn eq(&self, other: &PlayerId) -> bool {
        *self == other.0
    }
}
