//! Ghostty's terminal library, libghostty-vt, for the runtime and the terminal view.

pub mod input;
pub mod render;
mod restore;
mod snapshot;
mod terminal;

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]
#[rustfmt::skip]
mod sys;

#[cfg(test)]
mod testing;

pub use restore::Step;
pub use snapshot::Snapshot;
pub use terminal::{Cursor, Error, Terminal};

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use super::sys;

    fn build_info<T: Default>(key: sys::GhosttyBuildInfo) -> T {
        let mut value = T::default();
        let pointer = &mut value as *mut T as *mut c_void;
        let result = unsafe { sys::ghostty_build_info(key, pointer) };
        assert_eq!(result, sys::GhosttyResult_GHOSTTY_SUCCESS);
        value
    }

    #[test]
    fn the_linked_library_is_a_release_build_of_the_version_the_bindings_were_made_from() {
        let optimize: sys::GhosttyOptimizeMode =
            build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_OPTIMIZE);
        assert_eq!(
            optimize,
            sys::GhosttyOptimizeMode_GHOSTTY_OPTIMIZE_RELEASE_FAST
        );

        let simd: bool = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_SIMD);
        assert!(simd);

        let major: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_MAJOR);
        let minor: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_MINOR);
        let patch: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_PATCH);
        let linked = (major, minor, patch);
        let generated = (
            sys::GHOSTTY_VT_VERSION_MAJOR as usize,
            sys::GHOSTTY_VT_VERSION_MINOR as usize,
            sys::GHOSTTY_VT_VERSION_PATCH as usize,
        );
        assert_eq!(
            linked, generated,
            "src/sys.rs is from another version: run `cargo xtask ghostty-bindings`"
        );
    }
}
