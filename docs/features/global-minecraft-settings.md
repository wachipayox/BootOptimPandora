# Global Minecraft settings

One set of Minecraft settings (`options.txt`: video, sound, mouse, chat,
accessibility, skin parts, language and key bindings). **New instances are
created with them**, adapted to their Minecraft version and loader. After that,
each instance keeps its own `options.txt`: nothing is synced back and existing
instances are never rewritten on their own.

New instances means those created from scratch ("Nuevo") or from a global
profile (the carousel and `--create-instance`). Duplicated, imported and child
instances already bring their settings and are left alone.

## Where

- Launcher settings › General › Sincronización:
  - "Ajustes de Minecraft globales": on by default. Off, new instances start
    with Minecraft's or the profile's defaults.
  - "Gestionar": opens the editor (`pages/global_options_page.rs`). It has
    sections for video, sound, mouse and movement, chat, accessibility, skin,
    and language and game, plus key bindings (vanilla by category, mods by
    namespace, set by pressing the key or mouse button, Ctrl/Shift/Alt
    included) and "Otros ajustes" (read-only). Every setting has a "Copiar"
    switch; off, new instances keep their own value. Settings never set show
    Minecraft's default.
- Instance settings › Archivos compartidos › "Gestionar opciones globales"
  (`modals/global_options.rs`), both after a confirmation:
  - "Copiar los ajustes globales a esta instancia" overwrites its `options.txt`
    now.
  - "Usar los ajustes de esta instancia como globales" replaces the global
    settings (the "Copiar" switches are kept).
- The installer's migration makes the old launcher's `options.txt` the global
  settings.

With no global settings yet, nothing is copied.

## Flow

The store is `<launcher>/global_options.json` (`values`, `excluded`, `source`).
Creating an instance marks it `global_options_pending` in `info_v1.json` when
the feature is on and there is something to copy. Before its first start the
settings are written into `.minecraft/options.txt` and the mark is cleared. For
a Minecraft version that has never been used in the launcher and a profile
without its own `options.txt`, the format is not known yet: the copy then happens
when that first game exits.

Logic: `schema::global_options` (formats, tests) and
`crates/backend/src/global_options.rs` (store, copy).

## Formats

The canonical form is the modern one. The target's format is detected from its
own file first (which key it already uses and how its value looks); its data
version (`version:` line) is the fallback, and then the client jar's
`version.json`.

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
