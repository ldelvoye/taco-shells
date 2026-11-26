//! Looks still to be chosen between from pictures.

/// Where the banners sit: over the terminal, or in strips that push it down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BannerLayout {
    #[default]
    Cards,
    Strips,
}

/// Where the button for a new terminal in no group sits, if anywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlusPlace {
    #[default]
    Nowhere,
    Title,
    Head,
    Foot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trial {
    pub banners: BannerLayout,
    pub plus: PlusPlace,
}
