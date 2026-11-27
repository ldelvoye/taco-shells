use taco_identity::{Channel, DEVELOPMENT, Identity, STABLE};

pub fn identity_of(channel: Channel) -> &'static Identity {
    match channel {
        Channel::Stable => &STABLE,
        Channel::Development => &DEVELOPMENT,
    }
}

/// The icon a channel's bundle carries, made by `branding/build-icon.sh`.
pub fn icon_of(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "build/icon.icns",
        Channel::Development => "build/icon-dev.icns",
    }
}
