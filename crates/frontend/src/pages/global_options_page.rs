//! Launcher settings › Sincronización › Ajustes de Minecraft globales › Gestionar.
//!
//! Edits the launcher-wide Minecraft settings that new instances are created with
//! (`backend::global_options`). Values are canonical `options.txt` values (1.19+
//! encoding, GLFW key names); the backend adapts them to each instance. Every
//! setting has its own "Copiar" switch: switched off, new instances keep their
//! own value. Settings that were never set show Minecraft's default.
use std::collections::{BTreeMap, HashMap};

use bridge::{
    handle::BackendHandle,
    message::{GlobalOptionsSummary, MessageToBackend},
};
use gpui::{prelude::*, *};
use gpui_component::{
    button::ButtonVariants,
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
    spinner::Spinner,
    v_flex,
};

use crate::{component::design, entity::DataEntities, pages::page::Page, theme};

#[derive(Clone, Copy)]
enum Kind {
    Bool(bool),
    Choice(&'static [(&'static str, &'static str)], &'static str),
    Range {
        min: f32,
        max: f32,
        step: f32,
        default: f32,
        integer: bool,
        label: fn(f32) -> String,
    },
}

struct Spec {
    key: &'static str,
    label: &'static str,
    kind: Kind,
}

const fn bool_spec(key: &'static str, label: &'static str, default: bool) -> Spec {
    Spec {
        key,
        label,
        kind: Kind::Bool(default),
    }
}

const fn range(
    key: &'static str,
    label: &'static str,
    (min, max, step): (f32, f32, f32),
    default: f32,
    integer: bool,
    show: fn(f32) -> String,
) -> Spec {
    Spec {
        key,
        label,
        kind: Kind::Range {
            min,
            max,
            step,
            default,
            integer,
            label: show,
        },
    }
}

fn percent(v: f32) -> String {
    format!("{}%", (v * 100.).round() as i32)
}
fn chunks(v: f32) -> String {
    format!("{} chunks", v.round() as i32)
}
fn plain(v: f32) -> String {
    format!("{}", v.round() as i32)
}
fn decimal(v: f32) -> String {
    format!("{v:.1}").replace('.', ",")
}
fn fov(v: f32) -> String {
    format!("{}°", (70. + v * 40.).round() as i32)
}
fn sensitivity(v: f32) -> String {
    format!("{}%", (v * 200.).round() as i32)
}
fn fps(v: f32) -> String {
    if v >= 260. {
        "Ilimitados".into()
    } else {
        format!("{} fps", v.round() as i32)
    }
}
fn gui_scale(v: f32) -> String {
    if v < 0.5 { "Auto".into() } else { plain(v) }
}
fn biome_blend(v: f32) -> String {
    let n = v.round() as i32;
    if n == 0 {
        "No".into()
    } else {
        format!("{0}×{0}", 2 * n + 1)
    }
}
fn seconds(v: f32) -> String {
    if v < 0.05 {
        "Sin espera".into()
    } else {
        format!("{} s", decimal(v))
    }
}
fn times(v: f32) -> String {
    format!("{}×", decimal(v))
}

const SECTIONS: &[(&str, &[Spec])] = &[
    (
        "Vídeo",
        &[
            Spec {
                key: "graphicsMode",
                label: "Gráficos",
                kind: Kind::Choice(&[("0", "Rápidos"), ("1", "Detallados"), ("2", "Fabulosos")], "1"),
            },
            range("renderDistance", "Distancia de renderizado", (2., 32., 1.), 12., true, chunks),
            range("simulationDistance", "Distancia de simulación", (5., 32., 1.), 12., true, chunks),
            range("maxFps", "FPS máximos", (10., 260., 10.), 120., true, fps),
            bool_spec("enableVsync", "Sincronización vertical", true),
            bool_spec("fullscreen", "Pantalla completa", false),
            range("guiScale", "Tamaño de la interfaz", (0., 6., 1.), 0., true, gui_scale),
            range("fov", "Campo de visión", (-1., 1., 0.025), 0., false, fov),
            range("gamma", "Brillo", (0., 1., 0.01), 0.5, false, percent),
            Spec {
                key: "renderClouds",
                label: "Nubes",
                kind: Kind::Choice(
                    &[("\"true\"", "Detalladas"), ("\"fast\"", "Rápidas"), ("\"false\"", "No")],
                    "\"true\"",
                ),
            },
            Spec {
                key: "particles",
                label: "Partículas",
                kind: Kind::Choice(&[("0", "Todas"), ("1", "Reducidas"), ("2", "Mínimas")], "0"),
            },
            bool_spec("ao", "Iluminación suave", true),
            range("biomeBlendRadius", "Transición de biomas", (0., 7., 1.), 2., true, biome_blend),
            range("mipmapLevels", "Niveles de mipmap", (0., 4., 1.), 4., true, plain),
            bool_spec("entityShadows", "Sombras de entidades", true),
            range("entityDistanceScaling", "Distancia de entidades", (0.5, 5., 0.25), 1., false, percent),
            bool_spec("bobView", "Balanceo de la vista", true),
            Spec {
                key: "attackIndicator",
                label: "Indicador de ataque",
                kind: Kind::Choice(&[("0", "No"), ("1", "Mira"), ("2", "Barra")], "1"),
            },
            range("menuBackgroundBlurriness", "Desenfoque del menú", (0., 10., 1.), 5., true, plain),
            range("fovEffectScale", "Efectos de campo de visión", (0., 1., 0.01), 1., false, percent),
            range("screenEffectScale", "Efectos de distorsión", (0., 1., 0.01), 1., false, percent),
            range("darknessEffectScale", "Pulso de oscuridad", (0., 1., 0.01), 1., false, percent),
            range(
                "glintSpeed",
                "Velocidad del brillo de encantamientos",
                (0., 1., 0.01),
                0.5,
                false,
                percent,
            ),
            range(
                "glintStrength",
                "Intensidad del brillo de encantamientos",
                (0., 1., 0.01),
                0.75,
                false,
                percent,
            ),
        ],
    ),
    (
        "Sonido",
        &[
            range("soundCategory_master", "Volumen general", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_music", "Música", (0., 1., 0.01), 1., false, percent),
            range(
                "soundCategory_record",
                "Tocadiscos y bloques musicales",
                (0., 1., 0.01),
                1.,
                false,
                percent,
            ),
            range("soundCategory_weather", "Clima", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_block", "Bloques", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_hostile", "Criaturas hostiles", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_neutral", "Criaturas pacíficas", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_player", "Jugadores", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_ambient", "Ambiente", (0., 1., 0.01), 1., false, percent),
            range("soundCategory_voice", "Voces", (0., 1., 0.01), 1., false, percent),
            bool_spec("showSubtitles", "Subtítulos", false),
            bool_spec("directionalAudio", "Audio direccional", false),
        ],
    ),
    (
        "Ratón y movimiento",
        &[
            range("mouseSensitivity", "Sensibilidad", (0., 1., 0.005), 0.5, false, sensitivity),
            bool_spec("invertYMouse", "Invertir ratón", false),
            range("mouseWheelSensitivity", "Sensibilidad de la rueda", (1., 10., 0.5), 1., false, decimal),
            bool_spec("discrete_mouse_scroll", "Desplazamiento discreto", false),
            bool_spec("rawMouseInput", "Entrada directa del ratón", true),
            bool_spec("autoJump", "Salto automático", false),
            bool_spec("toggleCrouch", "Agacharse: alternar", false),
            bool_spec("toggleSprint", "Correr: alternar", false),
        ],
    ),
    (
        "Chat",
        &[
            Spec {
                key: "chatVisibility",
                label: "Chat",
                kind: Kind::Choice(&[("0", "Visible"), ("1", "Solo comandos"), ("2", "Oculto")], "0"),
            },
            bool_spec("chatColors", "Colores", true),
            bool_spec("chatLinks", "Enlaces", true),
            bool_spec("chatLinksPrompt", "Avisar al abrir enlaces", true),
            range("chatOpacity", "Opacidad del texto", (0.1, 1., 0.01), 1., false, percent),
            range("textBackgroundOpacity", "Opacidad del fondo", (0., 1., 0.01), 0.5, false, percent),
            range("chatScale", "Tamaño del texto", (0., 1., 0.01), 1., false, percent),
            range("chatWidth", "Ancho", (0., 1., 0.01), 1., false, percent),
            range("chatHeightFocused", "Alto (abierto)", (0., 1., 0.01), 1., false, percent),
            range("chatHeightUnfocused", "Alto (cerrado)", (0., 1., 0.01), 0.4375, false, percent),
            range("chatLineSpacing", "Interlineado", (0., 1., 0.01), 0., false, percent),
            range("chatDelay", "Retraso del chat", (0., 6., 0.1), 0., false, seconds),
            bool_spec("autoSuggestions", "Sugerencias de comandos", true),
            bool_spec("hideMatchedNames", "Ocultar nombres coincidentes", true),
            bool_spec("onlyShowSecureChat", "Solo chat seguro", false),
        ],
    ),
    (
        "Accesibilidad",
        &[
            Spec {
                key: "narrator",
                label: "Narrador",
                kind: Kind::Choice(&[("0", "No"), ("1", "Todo"), ("2", "Chat"), ("3", "Sistema")], "0"),
            },
            bool_spec("highContrast", "Alto contraste", false),
            bool_spec("hideLightningFlashes", "Ocultar destellos de rayos", false),
            bool_spec("darkMojangStudiosBackground", "Logotipo de carga monocromo", false),
            range("damageTiltStrength", "Inclinación al recibir daño", (0., 1., 0.01), 1., false, percent),
            range("notificationDisplayTime", "Duración de notificaciones", (0.5, 10., 0.5), 1., false, times),
            range("panoramaScrollSpeed", "Velocidad del panorama", (0., 1., 0.01), 1., false, percent),
            bool_spec("forceUnicodeFont", "Forzar fuente Unicode", false),
        ],
    ),
    (
        "Aspecto",
        &[
            Spec {
                key: "mainHand",
                label: "Mano principal",
                kind: Kind::Choice(&[("\"left\"", "Izquierda"), ("\"right\"", "Derecha")], "\"right\""),
            },
            bool_spec("modelPart_cape", "Capa", true),
            bool_spec("modelPart_jacket", "Chaqueta", true),
            bool_spec("modelPart_left_sleeve", "Manga izquierda", true),
            bool_spec("modelPart_right_sleeve", "Manga derecha", true),
            bool_spec("modelPart_left_pants_leg", "Pernera izquierda", true),
            bool_spec("modelPart_right_pants_leg", "Pernera derecha", true),
            bool_spec("modelPart_hat", "Sombrero", true),
        ],
    ),
    (
        "Idioma y juego",
        &[
            Spec {
                key: "lang",
                label: "Idioma",
                kind: Kind::Choice(
                    &[
                        ("es_es", "Español (España)"),
                        ("es_mx", "Español (México)"),
                        ("ca_es", "Català"),
                        ("gl_es", "Galego"),
                        ("eu_es", "Euskara"),
                        ("en_us", "English"),
                    ],
                    "en_us",
                ),
            },
            bool_spec("pauseOnLostFocus", "Pausar al cambiar de ventana", true),
            bool_spec("advancedItemTooltips", "Descripciones avanzadas (F3+H)", false),
        ],
    ),
];

/// Vanilla key bindings with their 1.21.1 defaults, by category.
const VANILLA_KEYS: &[(&str, &[(&str, &str, &str)])] = &[
    (
        "Movimiento",
        &[
            ("key_key.forward", "Adelante", "key.keyboard.w"),
            ("key_key.left", "Izquierda", "key.keyboard.a"),
            ("key_key.back", "Atrás", "key.keyboard.s"),
            ("key_key.right", "Derecha", "key.keyboard.d"),
            ("key_key.jump", "Saltar", "key.keyboard.space"),
            ("key_key.sneak", "Agacharse", "key.keyboard.left.shift"),
            ("key_key.sprint", "Correr", "key.keyboard.left.control"),
        ],
    ),
    (
        "Juego",
        &[
            ("key_key.attack", "Atacar o romper", "key.mouse.left"),
            ("key_key.use", "Usar o colocar", "key.mouse.right"),
            ("key_key.pickItem", "Elegir bloque", "key.mouse.middle"),
            ("key_key.drop", "Soltar objeto", "key.keyboard.q"),
            ("key_key.swapOffhand", "Cambiar de mano", "key.keyboard.f"),
        ],
    ),
    (
        "Inventario",
        &[
            ("key_key.inventory", "Inventario", "key.keyboard.e"),
            ("key_key.hotbar.1", "Barra rápida 1", "key.keyboard.1"),
            ("key_key.hotbar.2", "Barra rápida 2", "key.keyboard.2"),
            ("key_key.hotbar.3", "Barra rápida 3", "key.keyboard.3"),
            ("key_key.hotbar.4", "Barra rápida 4", "key.keyboard.4"),
            ("key_key.hotbar.5", "Barra rápida 5", "key.keyboard.5"),
            ("key_key.hotbar.6", "Barra rápida 6", "key.keyboard.6"),
            ("key_key.hotbar.7", "Barra rápida 7", "key.keyboard.7"),
            ("key_key.hotbar.8", "Barra rápida 8", "key.keyboard.8"),
            ("key_key.hotbar.9", "Barra rápida 9", "key.keyboard.9"),
            ("key_key.saveToolbarActivator", "Guardar barra", "key.keyboard.c"),
            ("key_key.loadToolbarActivator", "Cargar barra", "key.keyboard.x"),
        ],
    ),
    (
        "Multijugador",
        &[
            ("key_key.chat", "Abrir el chat", "key.keyboard.t"),
            ("key_key.command", "Escribir un comando", "key.keyboard.slash"),
            ("key_key.playerlist", "Lista de jugadores", "key.keyboard.tab"),
            ("key_key.socialInteractions", "Interacciones sociales", "key.keyboard.p"),
        ],
    ),
    (
        "Varios",
        &[
            ("key_key.screenshot", "Captura de pantalla", "key.keyboard.f2"),
            ("key_key.togglePerspective", "Cambiar perspectiva", "key.keyboard.f5"),
            ("key_key.smoothCamera", "Cámara cinemática", "key.keyboard.unknown"),
            ("key_key.fullscreen", "Pantalla completa", "key.keyboard.f11"),
            ("key_key.spectatorOutlines", "Resaltar jugadores (espectador)", "key.keyboard.unknown"),
            ("key_key.advancements", "Progresos", "key.keyboard.l"),
        ],
    ),
];

/// Settings never shown: they are never global (see `schema::global_options`).
const HIDDEN: &[&str] = &["version", "resourcePacks", "incompatibleResourcePacks", "lastServer"];

fn known_key(key: &str) -> bool {
    SECTIONS.iter().any(|(_, specs)| specs.iter().any(|spec| spec.key == key))
        || VANILLA_KEYS.iter().any(|(_, keys)| keys.iter().any(|(id, _, _)| *id == key))
        || HIDDEN.contains(&key)
}

/// "jei" for `key_key.jei.showRecipe`, "create" for `key_create.keyinfo.toolmenu`,
/// "xaero" for `key_gui.xaero_open_map`.
fn mod_of(key: &str) -> String {
    let id = key.trim_start_matches("key_");
    let id = id.strip_prefix("key.").unwrap_or(id);
    let id = id.strip_prefix("gui.").unwrap_or(id);
    let first = id.split(['.', '_', ' ']).next().unwrap_or(id);
    if first.is_empty() {
        "otros".into()
    } else {
        first.to_lowercase()
    }
}

/// A readable name for a mod's key binding id.
fn binding_label(key: &str) -> String {
    let id = key.trim_start_matches("key_");
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut words = String::new();
    for (index, c) in last.chars().enumerate() {
        if c == '_' {
            words.push(' ');
        } else if c.is_uppercase() && index > 0 {
            words.push(' ');
            words.extend(c.to_lowercase());
        } else {
            words.push(c);
        }
    }
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => id.to_string(),
    }
}

/// GLFW key name (`key.keyboard.left.shift`) to a short Spanish label.
fn key_name(binding: &str) -> String {
    let (key, modifier) = match binding.rsplit_once(':') {
        Some((key, m)) if matches!(m, "SHIFT" | "CONTROL" | "ALT" | "NONE") => (key, Some(m)),
        _ => (binding, None),
    };
    let base = if let Some(button) = key.strip_prefix("key.mouse.") {
        match button {
            "left" => "Clic izquierdo".to_string(),
            "right" => "Clic derecho".to_string(),
            "middle" => "Clic central".to_string(),
            other => format!("Botón {other} del ratón"),
        }
    } else if let Some(name) = key.strip_prefix("key.keyboard.") {
        match name {
            "unknown" => "Sin asignar".into(),
            "left.shift" => "Mayús izq.".into(),
            "right.shift" => "Mayús der.".into(),
            "left.control" => "Ctrl izq.".into(),
            "right.control" => "Ctrl der.".into(),
            "left.alt" => "Alt".into(),
            "right.alt" => "Alt Gr".into(),
            "space" => "Espacio".into(),
            "enter" => "Intro".into(),
            "escape" => "Esc".into(),
            "backspace" => "Retroceso".into(),
            "tab" => "Tabulador".into(),
            "caps.lock" => "Bloq Mayús".into(),
            "page.up" => "Re Pág".into(),
            "page.down" => "Av Pág".into(),
            "up" => "↑".into(),
            "down" => "↓".into(),
            "left" => "←".into(),
            "right" => "→".into(),
            "slash" => "/".into(),
            "comma" => ",".into(),
            "period" => ".".into(),
            "minus" => "-".into(),
            "equal" => "=".into(),
            "semicolon" => ";".into(),
            "apostrophe" => "'".into(),
            "grave.accent" => "`".into(),
            "backslash" => "\\".into(),
            "left.bracket" => "[".into(),
            "right.bracket" => "]".into(),
            other => match other.strip_prefix("keypad.") {
                Some(pad) => format!("Num {pad}"),
                None => other.to_uppercase(),
            },
        }
    } else {
        key.to_string()
    };
    match modifier {
        Some("SHIFT") => format!("Mayús + {base}"),
        Some("CONTROL") => format!("Ctrl + {base}"),
        Some("ALT") => format!("Alt + {base}"),
        _ => base,
    }
}

/// gpui's key (Windows virtual-key names and typed characters) to GLFW's name.
fn glfw_key(key: &str) -> Option<String> {
    let name = match key {
        "space" | "enter" | "tab" | "backspace" | "escape" | "insert" | "delete" | "home" | "end" | "up" | "down"
        | "left" | "right" => key.to_string(),
        "pageup" => "page.up".into(),
        "pagedown" => "page.down".into(),
        "-" => "minus".into(),
        "=" => "equal".into(),
        "[" => "left.bracket".into(),
        "]" => "right.bracket".into(),
        ";" => "semicolon".into(),
        "'" => "apostrophe".into(),
        "," => "comma".into(),
        "." => "period".into(),
        "/" => "slash".into(),
        "\\" => "backslash".into(),
        "`" => "grave.accent".into(),
        _ if key.len() <= 3 && key.starts_with('f') && key[1..].parse::<u8>().is_ok_and(|n| (1..=25).contains(&n)) => {
            key.to_string()
        },
        _ if key.chars().count() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()) => key.to_ascii_lowercase(),
        _ => return None,
    };
    Some(format!("key.keyboard.{name}"))
}

/// Minecraft writes floats with a decimal point (`1.0`).
fn format_value(value: f32, integer: bool) -> String {
    if integer {
        return format!("{}", value.round() as i64);
    }
    let rounded = (value as f64 * 10_000.).round() / 10_000.;
    let mut text = format!("{rounded}");
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

pub struct GlobalOptionsPage {
    backend: BackendHandle,
    summary: Option<GlobalOptionsSummary>,
    sliders: HashMap<&'static str, Entity<SliderState>>,
    /// Live slider values while dragging, shown before they are saved.
    dragging: HashMap<&'static str, f32>,
    capturing: Option<String>,
    capture_modifier: Option<&'static str>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _load: Task<()>,
}

impl GlobalOptionsPage {
    pub fn new(data: &DataEntities, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            backend: data.backend_handle.clone(),
            summary: None,
            sliders: HashMap::new(),
            dragging: HashMap::new(),
            capturing: None,
            capture_modifier: None,
            focus: cx.focus_handle(),
            _subscriptions: Vec::new(),
            _load: Task::ready(()),
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.backend.send(MessageToBackend::GetGlobalOptions { channel: send });
        self._load = cx.spawn(async move |page, cx| {
            let Ok(summary) = receive.await else {
                return;
            };
            let _ = page.update(cx, |page, cx| {
                page.build_sliders(&summary.values, cx);
                page.summary = Some(summary);
                cx.notify();
            });
        });
    }

    fn build_sliders(&mut self, values: &BTreeMap<String, String>, cx: &mut Context<Self>) {
        self.sliders.clear();
        self._subscriptions.clear();
        for (_, specs) in SECTIONS {
            for spec in specs.iter() {
                let Kind::Range {
                    min,
                    max,
                    step,
                    default,
                    integer,
                    ..
                } = spec.kind
                else {
                    continue;
                };
                let value = values
                    .get(spec.key)
                    .and_then(|value| value.trim().parse::<f32>().ok())
                    .unwrap_or(default)
                    .clamp(min, max);
                let state = cx.new(|_| SliderState::new().min(min).max(max).step(step).default_value(value));
                let key = spec.key;
                self._subscriptions.push(cx.subscribe(&state, move |page, _, event: &SliderEvent, cx| {
                    match event {
                        SliderEvent::Change(value) => {
                            page.dragging.insert(key, value.start());
                        },
                        SliderEvent::Release(value) => {
                            page.dragging.remove(key);
                            page.set(key, Some(format_value(value.start(), integer)));
                        },
                    }
                    cx.notify();
                }));
                self.sliders.insert(spec.key, state);
            }
        }
    }

    fn set(&mut self, key: &str, value: Option<String>) {
        if let Some(summary) = &mut self.summary {
            match &value {
                Some(value) => summary.values.insert(key.to_string(), value.clone()),
                None => summary.values.remove(key),
            };
        }
        self.backend.send(MessageToBackend::SetGlobalOption {
            key: key.to_string(),
            value,
        });
    }

    fn set_synced(&mut self, key: &str, synced: bool) {
        if let Some(summary) = &mut self.summary {
            if synced {
                summary.excluded.remove(key);
            } else {
                summary.excluded.insert(key.to_string());
            }
        }
        self.backend.send(MessageToBackend::SetGlobalOptionSynced {
            key: key.to_string(),
            synced,
        });
    }

    fn finish_capture(&mut self, binding: Option<String>, cx: &mut Context<Self>) {
        if let (Some(key), Some(binding)) = (self.capturing.take(), binding) {
            self.set(&key, Some(binding));
        }
        self.capturing = None;
        self.capture_modifier = None;
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.capturing.is_none() {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.finish_capture(None, cx);
            return;
        }
        let Some(name) = glfw_key(&keystroke.key) else {
            return;
        };
        let modifiers = keystroke.modifiers;
        let suffix = if modifiers.control {
            ":CONTROL"
        } else if modifiers.shift {
            ":SHIFT"
        } else if modifiers.alt {
            ":ALT"
        } else {
            ""
        };
        self.finish_capture(Some(format!("{name}{suffix}")), cx);
    }

    /// A modifier pressed and released alone binds that modifier (Shift to sneak, …).
    fn on_modifiers(&mut self, event: &ModifiersChangedEvent, cx: &mut Context<Self>) {
        if self.capturing.is_none() {
            return;
        }
        let modifiers = event.modifiers;
        let pressed = if modifiers.control {
            Some("key.keyboard.left.control")
        } else if modifiers.shift {
            Some("key.keyboard.left.shift")
        } else if modifiers.alt {
            Some("key.keyboard.left.alt")
        } else {
            None
        };
        match pressed {
            Some(name) => self.capture_modifier = Some(name),
            None => {
                if let Some(name) = self.capture_modifier.take() {
                    self.finish_capture(Some(name.to_string()), cx);
                }
            },
        }
    }

    fn on_mouse(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.capturing.is_none() {
            return;
        }
        let name = match event.button {
            MouseButton::Left => "key.mouse.left",
            MouseButton::Right => "key.mouse.right",
            MouseButton::Middle => "key.mouse.middle",
            MouseButton::Navigate(NavigationDirection::Back) => "key.mouse.4",
            MouseButton::Navigate(NavigationDirection::Forward) => "key.mouse.5",
            #[allow(unreachable_patterns)]
            _ => return,
        };
        self.finish_capture(Some(name.to_string()), cx);
    }

    fn copy_switch(&self, key: String, cx: &mut Context<Self>) -> AnyElement {
        let synced = self.summary.as_ref().is_none_or(|summary| !summary.excluded.contains(&key));
        design::chip(SharedString::from(format!("copy-{key}")), "Copiar", synced)
            .h(px(26.))
            .px(px(9.))
            .tooltip(if synced {
                "Las instancias nuevas reciben este ajuste"
            } else {
                "Las instancias nuevas mantienen su propio valor"
            })
            .on_click(cx.listener(move |page, _, _, cx| {
                page.set_synced(&key, !synced);
                cx.notify();
            }))
            .into_any_element()
    }

    fn row(&self, label: impl Into<SharedString>, control: AnyElement, copy: AnyElement) -> Div {
        h_flex()
            .w_full()
            .min_h(px(40.))
            .gap(px(16.))
            .items_center()
            .child(div().flex_1().min_w_0().text_size(px(14.)).child(label.into()))
            .child(control)
            .child(copy)
    }

    fn render_spec(&self, spec: &'static Spec, cx: &mut Context<Self>) -> Div {
        let value = self.summary.as_ref().and_then(|summary| summary.values.get(spec.key).cloned());
        let control = match spec.kind {
            Kind::Bool(default) => {
                let checked = value.as_deref().map_or(default, |value| value.trim() == "true");
                let key = spec.key;
                design::YesNo::new(SharedString::from(format!("option-{key}")))
                    .checked(checked)
                    .on_click(cx.listener(move |page, value: &bool, _, cx| {
                        page.set(key, Some(value.to_string()));
                        cx.notify();
                    }))
                    .into_any_element()
            },
            Kind::Choice(options, default) => {
                let current = value.unwrap_or_else(|| default.to_string());
                let key = spec.key;
                let mut chips = h_flex().gap(px(4.)).flex_wrap().justify_end().max_w(px(420.));
                for (index, (option, label)) in options.iter().enumerate() {
                    let selected = current == *option;
                    let option = option.to_string();
                    chips = chips.child(
                        design::chip(SharedString::from(format!("option-{key}-{index}")), *label, selected)
                            .h(px(28.))
                            .on_click(cx.listener(move |page, _, _, cx| {
                                page.set(key, Some(option.clone()));
                                cx.notify();
                            })),
                    );
                }
                chips.into_any_element()
            },
            Kind::Range {
                min,
                max,
                default,
                label,
                ..
            } => {
                let shown = self.dragging.get(spec.key).copied().unwrap_or_else(|| {
                    value
                        .as_deref()
                        .and_then(|value| value.trim().parse::<f32>().ok())
                        .unwrap_or(default)
                        .clamp(min, max)
                });
                let slider = match self.sliders.get(spec.key) {
                    Some(state) => Slider::new(state).w(px(220.)).into_any_element(),
                    None => div().w(px(220.)).into_any_element(),
                };
                h_flex()
                    .gap(px(12.))
                    .items_center()
                    .child(slider)
                    .child(div().w(px(96.)).text_size(px(13.)).text_color(rgb(theme::INK_MUTED)).child(label(shown)))
                    .into_any_element()
            },
        };
        self.row(spec.label, control, self.copy_switch(spec.key.to_string(), cx))
    }

    fn render_binding(&self, key: String, label: String, default: &str, cx: &mut Context<Self>) -> Div {
        let value = self.summary.as_ref().and_then(|summary| summary.values.get(&key).cloned());
        let binding = value.unwrap_or_else(|| default.to_string());
        let capturing = self.capturing.as_deref() == Some(key.as_str());
        let capture_key = key.clone();
        let button = design::DesignButton::new(SharedString::from(format!("bind-{key}")))
            .w(px(200.))
            .h(px(30.))
            .toggled(capturing)
            .label(if capturing {
                "Pulsa una tecla…".to_string()
            } else {
                key_name(&binding)
            })
            .on_click(cx.listener(move |page, _, window, cx| {
                page.capturing = Some(capture_key.clone());
                page.capture_modifier = None;
                window.focus(&page.focus, cx);
                cx.notify();
            }));
        let clear_key = key.clone();
        let clear = design::DesignButton::new(SharedString::from(format!("unbind-{key}")))
            .ghost()
            .h(px(30.))
            .px(px(8.))
            .label("Quitar")
            .tooltip("Sin asignar")
            .on_click(cx.listener(move |page, _, _, cx| {
                page.set(&clear_key, Some("key.keyboard.unknown".into()));
                cx.notify();
            }));
        self.row(
            label,
            h_flex().gap(px(4.)).child(button).child(clear).into_any_element(),
            self.copy_switch(key, cx),
        )
    }

    fn panel(title: &str) -> Div {
        v_flex().w_full().gap(px(4.)).px(px(18.)).py(px(16.)).bg(rgb(theme::PANEL)).child(
            div()
                .pb(px(6.))
                .font_family(theme::DISPLAY)
                .font_weight(FontWeight::BOLD)
                .text_size(px(20.))
                .line_height(px(24.))
                .child(title.to_string()),
        )
    }

    fn subheading(title: impl Into<SharedString>) -> Div {
        div().pt(px(10.)).pb(px(2.)).child(design::eyebrow(title))
    }
}

impl Page for GlobalOptionsPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        true
    }
}

impl Render for GlobalOptionsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(summary) = self.summary.clone() else {
            return v_flex().w_full().p(px(24.)).child(Spinner::new());
        };
        let intro = if summary.values.is_empty() {
            "Todavía no hay ajustes globales, así que se muestran los de Minecraft por defecto. Cambia los que quieras, o usa los de una instancia desde sus ajustes › Gestionar opciones globales.".to_string()
        } else {
            match &summary.source {
                Some(source) => format!("Tomados de «{source}» y de lo que hayas cambiado aquí."),
                None => "Los que has elegido aquí.".to_string(),
            }
        };
        let mut content = v_flex()
            .w_full()
            .max_w(px(980.))
            .gap(px(16.))
            .p(px(20.))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .child("Las instancias nuevas se crean con estos ajustes de Minecraft, adaptados a su versión y su loader. Después, cada instancia guarda los suyos. Desactiva «Copiar» en un ajuste para que cada instancia nueva conserve su propio valor."),
                    )
                    .child(div().text_size(px(13.)).text_color(rgb(theme::INK_MUTED)).child(intro)),
            );

        for (title, specs) in SECTIONS {
            let mut panel = Self::panel(title);
            for spec in specs.iter() {
                panel = panel.child(self.render_spec(spec, cx));
            }
            content = content.child(panel);
        }

        let mut keys = Self::panel("Teclas");
        for (category, bindings) in VANILLA_KEYS {
            keys = keys.child(Self::subheading(*category));
            for (key, label, default) in bindings.iter() {
                keys = keys.child(self.render_binding((*key).to_string(), (*label).to_string(), default, cx));
            }
        }
        let mut by_mod: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for key in summary.values.keys().filter(|key| key.starts_with("key_") && !known_key(key)) {
            by_mod.entry(mod_of(key)).or_default().push(key.clone());
        }
        for (mod_id, bindings) in by_mod {
            keys = keys.child(Self::subheading(mod_id));
            for key in bindings {
                let label = binding_label(&key);
                keys = keys.child(self.render_binding(key, label, "key.keyboard.unknown", cx));
            }
        }
        content = content.child(keys);

        let others: Vec<(String, String)> = summary
            .values
            .iter()
            .filter(|(key, _)| !key.starts_with("key_") && !known_key(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if !others.is_empty() {
            let mut panel = Self::panel("Otros ajustes").child(
                div()
                    .pb(px(6.))
                    .text_size(px(13.))
                    .text_color(rgb(theme::INK_MUTED))
                    .child("Ajustes sin editor propio, tal y como están en el options.txt."),
            );
            for (key, value) in others {
                panel = panel.child(
                    self.row(
                        key.clone(),
                        div()
                            .max_w(px(320.))
                            .truncate()
                            .text_size(px(13.))
                            .text_color(rgb(theme::INK_MUTED))
                            .child(value)
                            .into_any_element(),
                        self.copy_switch(key, cx),
                    ),
                );
            }
            content = content.child(panel);
        }

        v_flex()
            .w_full()
            .items_center()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|page, event: &KeyDownEvent, _, cx| page.on_key(event, cx)))
            .on_modifiers_changed(
                cx.listener(|page, event: &ModifiersChangedEvent, _, cx| page.on_modifiers(event, cx)),
            )
            .when(self.capturing.is_some(), |root| {
                root.on_any_mouse_down(cx.listener(|page, event: &MouseDownEvent, _, cx| page.on_mouse(event, cx)))
            })
            .child(content)
    }
}
