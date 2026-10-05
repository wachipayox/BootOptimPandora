use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, Sizable, StyledExt, button::Button, checkbox::Checkbox, h_flex, input::Input, scroll::ScrollableElement, v_flex,
};

use super::{SettingGroup, SettingItem, SettingItemWidget, SettingPage, SettingsRoot};
use crate::icon::PandoraIcon;

pub(super) fn create_page() -> SettingPage {
    SettingPage {
        title: || "Rutas ignoradas",
        groups: vec![SettingGroup {
            title: None,
            items: vec![SettingItem {
                title: || "Archivos ignorados del modpack",
                description: || "Rutas de .minecraft omitidas al buscar cambios y heredar archivos.",
                widget: SettingItemWidget::BackendWide(Rc::new(|config, root, window, cx| {
                    let muted = cx.theme().muted_foreground;
                    let border = cx.theme().border;
                    let list_height = (window.viewport_size().height.as_f32() - 365.0).max(160.0);
                    let other_enabled = config.ignored_profile_paths.0.iter().any(|path| path == "*");
                    let path_count = config.ignored_profile_paths.0.iter().filter(|path| path.as_str() != "*").count();
                    let mut list = v_flex().gap_1();
                    for path in config.ignored_profile_paths.0.iter().filter(|path| path.as_str() != "*") {
                        let remove = path.clone();
                        let label = format!("/{path}");
                        list = list.child(h_flex().w_full().items_center().justify_between().gap_3()
                            .px_3().py_2().rounded_md().border_1().border_color(border)
                            .child(h_flex().items_center().gap_2().min_w_0()
                                .child(Icon::new(PandoraIcon::Folder).size_4().text_color(muted))
                                .child(div().font_family("Consolas").text_sm().truncate().child(label)))
                            .child(Button::new(SharedString::from(format!("unignore-{path}")))
                                .label("Quitar").small()
                                .on_click(cx.listener(move |root, _, _, cx| root.remove_ignored_path(&remove, cx)))));
                    }
                    if path_count == 0 {
                        list = list.child(div().p_4().text_sm().text_color(muted)
                            .child("La lista está vacía. Las rutas protegidas del juego siguen excluidas."));
                    }
                    v_flex().w_full().gap_4()
                        .child(v_flex().gap_1()
                            .child(div().text_lg().font_semibold().child("Rutas ignoradas"))
                            .child(div().text_sm().text_color(muted)
                                .child("Se aplican a todas las instancias. Una carpeta incluye recursivamente su contenido. Las rutas empiezan en .minecraft; ignorarlas no borra archivos.")))
                        .child(v_flex().gap_2().px_3().py_2().rounded_md().border_1().border_color(border)
                            .child(Checkbox::new("ignore-other-profile-paths")
                                .label("Ignorar Otras rutas").checked(other_enabled)
                                .on_click(cx.listener(|root, enabled, _, cx| root.set_other_paths_ignored(*enabled, cx))))
                            .child(div().text_xs().text_color(muted)
                                .child("Incluye fancymenu_data, options.txt, optionsviveprofiles.txt y otras rutas fuera del contenido habitual del modpack.")))
                        .child(h_flex().w_full().gap_2().items_center()
                            .child(Input::new(&root.ignored_path_input).flex_1())
                            .child(Button::new("add-ignored-profile-path").label("Añadir ruta")
                                .on_click(cx.listener(|root, _, window, cx| root.add_ignored_path(window, cx)))))
                        .when_some(root.ignored_path_error.clone(), |view, error| {
                            view.child(div().text_sm().text_color(cx.theme().danger).child(error))
                        })
                        .child(div().text_xs().text_color(muted)
                            .child("Ejemplos: /mods/.connector, /.analogaudio, /config/client-cache"))
                        .child(div().text_sm().font_semibold()
                            .child(format!("{path_count} rutas específicas")))
                        .when((path_count as f32 * 56.0) > list_height, |view| {
                            view.child(div().text_xs().text_color(muted)
                                .child("Desplaza la lista para ver las demás reglas."))
                        })
                        .child(v_flex().id("ignored-profile-path-list")
                            .max_h(px(list_height)).pr_3().overflow_y_scrollbar().child(list))
                        .into_any_element()
                })),
                ..Default::default()
            }].into(),
            searched_items: None,
        }].into(),
        searched_groups: None,
    }
}

impl SettingsRoot {
    fn set_other_paths_ignored(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let mut paths = self.backend_config().map(|config| config.ignored_profile_paths.0.clone()).unwrap_or_default();
        paths.retain(|path| path != "*");
        if enabled { paths.push("*".into()); }
        self.set_ignored_profile_paths(paths, cx);
    }

    pub(super) fn add_ignored_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.ignored_path_input.read(cx).value().to_string();
        let mut paths = self
            .backend_config()
            .map(|config| config.ignored_profile_paths.0.clone())
            .unwrap_or_default();
        paths.push(path);
        if self.set_ignored_profile_paths(paths, cx) {
            self.ignored_path_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    pub(super) fn remove_ignored_path(&mut self, path: &str, cx: &mut Context<Self>) {
        let mut paths = self
            .backend_config()
            .map(|config| config.ignored_profile_paths.0.clone())
            .unwrap_or_default();
        paths.retain(|item| item != path);
        self.set_ignored_profile_paths(paths, cx);
    }
}
