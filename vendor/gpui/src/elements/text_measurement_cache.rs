//! Exact constraints for reusing a text measurement within one layout pass.
pub(crate) fn can_reuse<T: Copy + PartialEq>(
    wrap: Option<T>,
    truncate: Option<T>,
    cached_wrap: Option<T>,
    cached_truncate: Option<T>,
) -> bool {
    if truncate.is_some() {
        // A bounded answer is valid only for these same two constraints.
        truncate == cached_truncate && wrap == cached_wrap
    } else {
        // Keep the existing untruncated policy, including intrinsic probes.
        cached_truncate.is_none() && (wrap.is_none() || wrap == cached_wrap)
    }
}

#[cfg(test)]
mod tests {
    use super::can_reuse;

    #[test]
    fn repeated_bounded_measurement_is_reused() {
        assert!(can_reuse(Some(180), Some(180), Some(180), Some(180)));
        assert!(can_reuse(None, Some(300), None, Some(300)));
    }

    #[test]
    fn resizing_or_changing_wrap_requires_a_fresh_measurement() {
        for width in [80, 179, 181, 480] {
            assert!(!can_reuse(Some(width), Some(width), Some(180), Some(180)));
            assert!(!can_reuse(None, Some(width), None, Some(180)));
        }
        assert!(!can_reuse(None, Some(180), Some(180), Some(180)));
        assert!(!can_reuse(Some(180), Some(180), None, Some(180)));
    }

    #[test]
    fn intrinsic_probe_never_reuses_a_truncated_answer() {
        assert!(!can_reuse(None, None, None, Some(180)));
        assert!(!can_reuse(None, None, Some(180), Some(180)));
        assert!(!can_reuse(None, Some(180), None, None));
        assert!(can_reuse::<u32>(None, None, None, None));
        assert!(can_reuse(Some(180), None, Some(180), None));
        assert!(!can_reuse(Some(181), None, Some(180), None));
    }
}
