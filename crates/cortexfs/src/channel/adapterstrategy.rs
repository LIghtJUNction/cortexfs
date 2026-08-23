use crate::is_object_name;

/// Selects the platform adapter for one channel instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterStrategy {
    /// Use a catalog family such as `nostr` or `discord`.
    Catalog(String),
    /// Run `channel/<name>.d/adapter.d/<name>` as a custom socket driver.
    Custom(String),
}

impl AdapterStrategy {
    /// Parses `channel/<name>.d/adapter`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if cortexfs_channels::platform::catalog::find(value).is_some() {
            return Some(Self::Catalog(value.to_owned()));
        }
        if is_object_name(value) {
            return Some(Self::Custom(value.to_owned()));
        }
        None
    }

    /// Derives a catalog family from a channel id such as `telegram.primary`.
    #[must_use]
    pub fn family_from_channel_id(channel: &str) -> Option<String> {
        let family = channel.split_once('.').map_or(channel, |(family, _)| family);
        cortexfs_channels::platform::catalog::find(family)
            .map(|spec| spec.id.to_owned())
            .or_else(|| is_object_name(family).then(|| family.to_owned()))
    }
}
