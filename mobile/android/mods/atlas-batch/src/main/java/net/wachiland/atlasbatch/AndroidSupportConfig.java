package net.wachiland.atlasbatch;

import com.mojang.logging.LogUtils;
import java.io.IOException;
import java.io.Reader;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Properties;
import net.neoforged.fml.loading.FMLPaths;

/** Conservative defaults: diagnostics and unvalidated mechanisms are opt-in. */
final class AndroidSupportConfig {
    static final boolean ANDROID=System.getProperty("os.version","").startsWith("Android")
        || System.getenv("MOD_ANDROID_RUNTIME")!=null;
    static volatile boolean diagnostics;
    static volatile boolean enabled=true;
    static volatile boolean stagedUploads;
    static void load() {
        Properties values=new Properties();
        Path file=FMLPaths.CONFIGDIR.get().resolve("wachiland-android-support.properties");
        try {
            if (Files.exists(file)) {
                try (Reader in=Files.newBufferedReader(file,StandardCharsets.UTF_8)) { values.load(in); }
            } else {
                Files.createDirectories(file.getParent());
                values.setProperty("enabled","true");
                values.setProperty("diagnostics","false");
                values.setProperty("experimental.stagedUploads","false");
                try (Writer out=Files.newBufferedWriter(file,StandardCharsets.UTF_8)) {
                    values.store(out,"Android support foundation. Quality is unchanged. Diagnostics add measurement overhead. No automatic mod removal or unvalidated GPU optimization.");
                }
            }
            enabled=bool(values,"enabled",true);
            diagnostics=bool(values,"diagnostics",false);
            stagedUploads=bool(values,"experimental.stagedUploads",false);
        } catch (IOException | IllegalArgumentException e) {
            enabled=false; diagnostics=false; stagedUploads=false;
            LogUtils.getLogger().warn("[Wachiland Android support] Configuration unavailable; stock behavior",e);
        }
    }
    private static boolean bool(Properties values,String name,boolean fallback) {
        String value=values.getProperty(name);
        if (value==null) return fallback;
        if ("true".equalsIgnoreCase(value.trim())) return true;
        if ("false".equalsIgnoreCase(value.trim())) return false;
        throw new IllegalArgumentException("Expected true/false for "+name);
    }
    static boolean observe() { return enabled && ANDROID && diagnostics; }
}
