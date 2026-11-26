pub const CHANNEL_VARIABLE: &str = "TACO_SHELLS_CHANNEL";
pub const STABLE_CHANNEL: &str = "stable";

pub struct Identity {
    pub product_name: &'static str,
    pub bundle_id: &'static str,
    pub icon: &'static str,
}

pub const STABLE: Identity = Identity {
    product_name: "Taco Shells",
    bundle_id: "com.ldelvoye.tacoshells",
    icon: "build/icon.icns",
};

pub const DEVELOPMENT: Identity = Identity {
    product_name: "Taco Shells Dev",
    bundle_id: "com.ldelvoye.tacoshells.dev",
    icon: "build/icon-dev.icns",
};
