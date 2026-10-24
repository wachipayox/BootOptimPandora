//! Recommended heap calculation, shared by launch and local presentation.
pub(crate) fn recommendation() -> schema::instance::MemoryRecommendation {
    schema::instance::MemoryRecommendation::for_total_mib(
        super::profile_jvm::total_memory_mib().and_then(|n| u64::try_from(n).ok()),
    )
}
