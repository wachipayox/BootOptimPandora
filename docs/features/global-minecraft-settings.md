# Global Minecraft settings

One set of Minecraft settings (`options.txt`: video, sound, controls,
accessibility, skin parts…) for every instance. Unlike the old shared
`options.txt`, the file is never shared: each instance keeps its own, and the
launcher rewrites the global settings into it in the format that its Minecraft
version and loader expect.

It is off by default. It is turned on by the launcher setting "Ajustes de
Minecraft globales", or by the installer's migration, which takes the old
launcher's `options.txt` as the global settings. An instance can opt out in its
settings ("Usar los ajustes de Minecraft globales"), which also has "Usar los
ajustes de esta instancia como globales". An instance whose `options.txt` is in
the sync targets is left to the syncing.

## Flow

| When | What |
|---|---|
| Before the game starts | Changes made while the launcher was not watching are kept, then the global settings are written into `.minecraft/options.txt`. What the file looks like afterwards is saved in `<instance>/.pandora-global-options-v1.json`. |
| When the game exits | Every setting the player changed since then becomes global, and the instance is brought up to date again. |
| Empty global settings | The first instance started fills them, so turning the feature on keeps the player's settings. |

Only settings that were there when the launcher last applied are compared. Keys
that appear later (a new mod's key bindings, defaults the game writes) are not
preferences and stay local until the player changes them.

The store is `<launcher>/global_options.json`. Logic: `schema::global_options`
(format, tests) and `crates/backend/src/global_options.rs` (hooks, store).

## Formats

The canonical form is the modern one. The target's format is detected from its
own file first (which key it already uses and how its value looks); its data
version (`version:` line) is the fallback. A new instance without an
`options.txt` gets the data version from its client jar (`version.json`), or
waits until the game has written the file once.

| Setting | Detected forms |
|---|---|
| Graphics | `fancyGraphics:true/false` (before 1.16), `graphicsMode:0/1/2`, `graphicsPreset:"…"` (newest versions; `graphicsMode` is not written there) |
| Ambient occlusion | `ao:0/1/2` (before 1.19), `ao:true/false` |
| `renderClouds`, `mainHand`, `soundDevice`, `graphicsPreset`, `inactivityFpsLimit` | Quoted JSON strings from 1.19, bare words before |
| Language | `en_US` before 1.11, `en_us` after |
| Key bindings | LWJGL 2 key codes (before 1.13) or GLFW names. Forge/NeoForge `:SHIFT`/`:CONTROL`/`:ALT` modifiers; a modified binding is not written to Fabric or Vanilla, which would drop the modifier |
| `gamma` | Clamped to 1.0 from 1.19, which resets larger values |

Never global: `version`, `resourcePacks`, `incompatibleResourcePacks` and
`lastServer`. Unknown keys are carried as they are; older versions ignore keys
they do not know.
