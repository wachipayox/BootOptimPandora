package net.wachiland.atlasbatch;

/** No retained snapshots or positions: only bounded capacity prediction. */
public final class EntityCullingAllocationPolicy {
    private EntityCullingAllocationPolicy() { }
    public static boolean active() {
        return AndroidSupportConfig.ANDROID && AndroidSupportConfig.enabled
            && AndroidSupportConfig.entityCullingAllocation;
    }
}
