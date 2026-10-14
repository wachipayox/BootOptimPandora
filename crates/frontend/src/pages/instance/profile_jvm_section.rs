//! Instance settings: the profile's distributed JVM options, separate from the player's own JVM
//! flags. Each option is a checkbox (or Automatic/On/Off when the profile can decide from the
//! machine) with a "?" tooltip, plus a number field or dropdown for its value. The status line
//! shows what the next Start does with it, as decided by the backend.

use std::collections::{BTreeMap, HashMap};

use bridge::{handle::BackendHandle, instance::InstanceID, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    select::{Select, SelectEvent, SelectState},
    v_flex,
};
use schema::profile_jvm::{
    JvmOptionKind, JvmPlanEntry, JvmPlanStatus, JvmValue, ManifestJvmOption, ProfileJvmOptionChoice,
    ProfileJvmOptionsView,
};

const PLATFORM: &str = "pc";

type ChoiceSelect = Entity<SelectState<Vec<SharedString>>>;

#[derive(Default)]
struct OptionControls {
    number: Option<Entity<InputState>>,
    values: Option<ChoiceSelect>,
    mode: Option<ChoiceSelect>,
}

pub struct ProfileJvmSection {
    backend_handle: BackendHandle,
    instance_id: InstanceID,
    view: Option<ProfileJvmOptionsView>,
    choices: BTreeMap<String, ProfileJvmOptionChoice>,
    controls: HashMap<String, OptionControls>,
    _load: Task<()>,
}

impl ProfileJvmSection {
    pub fn new(
        backend_handle: BackendHandle,
        instance_id: InstanceID,
        choices: BTreeMap<String, ProfileJvmOptionChoice>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut section = Self {
            backend_handle,
            instance_id,
            view: None,
            choices,
            controls: HashMap::new(),
            _load: Task::ready(()),
        };
        section.refresh(cx);
        section
    }

    /// Asks the backend again, for example after the player's JVM flags or memory changed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (send, recv) = tokio::sync::oneshot::channel();
        self.backend_handle.send(MessageToBackend::GetProfileJvmOptions {
            id: self.instance_id,
            channel: send,
        });
        self._load = cx.spawn(async move |section, cx| {
            let Ok(view) = recv.await else {
                return;
            };
            _ = section.update(cx, |section, cx| {
                section.view = view;
                cx.notify();
            });
        });
    }

    fn set_choice(&mut self, id: &str, update: impl FnOnce(&mut ProfileJvmOptionChoice), cx: &mut Context<Self>) {
        let choice = self.choices.entry(id.to_owned()).or_default();
        update(choice);
        let choice = choice.clone();
        if choice.is_untouched() {
            self.choices.remove(id);
        }
        self.backend_handle.send(MessageToBackend::SetInstanceProfileJvmOption {
            id: self.instance_id,
            option_id: id.to_owned(),
            choice,
        });
        self.refresh(cx);
    }

    fn reset(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_choice(id, |choice| *choice = ProfileJvmOptionChoice::default(), cx);
        // Recreate the value controls so they show the profile value again.
        self.controls.remove(id);
        window.refresh();
    }

    fn ensure_controls(&mut self, option: &ManifestJvmOption, window: &mut Window, cx: &mut Context<Self>) {
        if self.controls.contains_key(&option.id) {
            return;
        }
        let Some(platform) = option.platforms.get(PLATFORM) else {
            return;
        };
        let choice = self.choices.get(&option.id).cloned().unwrap_or_default();
        let value = choice.value.clone().or_else(|| platform.default_value.clone());
        let mut controls = OptionControls::default();
        let id = option.id.clone();

        match option.kind {
            JvmOptionKind::Int => {
                let text = value.map(|value| value.to_string()).unwrap_or_default();
                let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
                let (min, max) = (option.min.unwrap_or(i64::MIN), option.max.unwrap_or(i64::MAX));
                cx.subscribe_in(&state, window, move |_, state, event: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = event;
                    let Ok(current) = state.read(cx).value().trim().parse::<i64>() else {
                        return;
                    };
                    let next = match action {
                        StepAction::Increment => current.saturating_add(1),
                        StepAction::Decrement => current.saturating_sub(1),
                    }
                    .clamp(min, max);
                    state.update(cx, |input, cx| input.set_value(next.to_string(), window, cx));
                })
                .detach();
                let option_id = id.clone();
                cx.subscribe(&state, move |section, state, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    // Only values inside the profile's range are stored; others are ignored.
                    if let Ok(number) = state.read(cx).value().trim().parse::<i64>()
                        && (min..=max).contains(&number)
                    {
                        section.set_choice(&option_id, |choice| choice.value = Some(JvmValue::Int(number)), cx);
                    }
                })
                .detach();
                controls.number = Some(state);
            },
            JvmOptionKind::Enum => {
                let items = option.values.iter().cloned().map(SharedString::from).collect::<Vec<_>>();
                let selected = value.map(|value| SharedString::from(value.to_string()));
                let state = cx.new(|cx| {
                    let mut state = SelectState::new(items, None, window, cx);
                    if let Some(selected) = &selected {
                        state.set_selected_value(selected, window, cx);
                    }
                    state
                });
                let option_id = id.clone();
                cx.subscribe(&state, move |section, _, event: &SelectEvent<Vec<SharedString>>, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    let value = value.to_string();
                    section.set_choice(&option_id, |choice| choice.value = Some(JvmValue::Text(value)), cx);
                })
                .detach();
                controls.values = Some(state);
            },
            JvmOptionKind::Bool => {},
        }

        if platform.auto.is_some() {
            let items: Vec<SharedString> = vec![
                t::instance::profile_jvm::automatic().into(),
                t::instance::profile_jvm::on().into(),
                t::instance::profile_jvm::off().into(),
            ];
            let selected = items[match choice.enabled {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            }]
            .clone();
            let state = cx.new(|cx| {
                let mut state = SelectState::new(items.clone(), None, window, cx);
                state.set_selected_value(&selected, window, cx);
                state
            });
            let option_id = id.clone();
            cx.subscribe(&state, move |section, _, event: &SelectEvent<Vec<SharedString>>, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                let enabled = if *value == items[1] {
                    Some(true)
                } else if *value == items[2] {
                    Some(false)
                } else {
                    None
                };
                section.set_choice(&option_id, |choice| choice.enabled = enabled, cx);
            })
            .detach();
            controls.mode = Some(state);
        }
        self.controls.insert(id, controls);
    }
}

fn status_text(entry: &JvmPlanEntry, java_major: Option<u32>) -> SharedString {
    match &entry.status {
        JvmPlanStatus::Applied { arg, .. } => t::instance::profile_jvm::status_applied(arg).into(),
        JvmPlanStatus::Disabled { .. } => t::instance::profile_jvm::status_off().into(),
        JvmPlanStatus::Overridden { .. } => t::instance::profile_jvm::status_overridden().into(),
        JvmPlanStatus::Skipped { reason } if reason == "java-version-unknown" || reason == "java-vm-unknown" => {
            t::instance::profile_jvm::status_java_pending().into()
        },
        JvmPlanStatus::Skipped { reason } => {
            let detail = match (reason.strip_prefix("requires-java-"), java_major) {
                (Some(range), Some(major)) => format!("requires Java {range} (this instance: {major})"),
                _ => reason.clone(),
            };
            t::instance::profile_jvm::status_not_used(&detail).into()
        },
    }
}

fn effective_enabled(entry: &JvmPlanEntry, choice: Option<&ProfileJvmOptionChoice>, default: bool) -> bool {
    match &entry.status {
        JvmPlanStatus::Applied { .. } | JvmPlanStatus::Overridden { .. } => true,
        JvmPlanStatus::Disabled { .. } => false,
        JvmPlanStatus::Skipped { .. } => choice.and_then(|choice| choice.enabled).unwrap_or(default),
    }
}

fn tooltip_text(option: &ManifestJvmOption) -> SharedString {
    let mut text = option.help.clone();
    if let Some(platform) = option.platforms.get(PLATFORM) {
        let arg = schema::profile_jvm::render(option, platform.default_value.as_ref()).unwrap_or_default();
        text.push_str("\n\n");
        text.push_str(&t::instance::profile_jvm::argument(&arg));
        text.push('\n');
        text.push_str(if platform.default_enabled {
            t::instance::profile_jvm::default_on()
        } else {
            t::instance::profile_jvm::default_off()
        });
        if let Some(summary) = &platform.evidence.summary {
            text.push('\n');
            text.push_str(summary);
        }
    }
    text.into()
}

impl Render for ProfileJvmSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(view) = self.view.clone() else {
            return div().into_any_element();
        };
        let options = view
            .options
            .into_iter()
            .filter(|(option, _)| option.platforms.contains_key(PLATFORM))
            .collect::<Vec<_>>();
        if options.is_empty() {
            return div().into_any_element();
        }
        let java_major = view.facts.java_major;

        let mut list = v_flex()
            .gap_2()
            .child(div().font_bold().child(t::instance::profile_jvm::title()))
            .child(div().text_sm().child(t::instance::profile_jvm::description()));

        for (option, entry) in options {
            self.ensure_controls(&option, window, cx);
            let platform = &option.platforms[PLATFORM];
            let choice = self.choices.get(&option.id).cloned();
            let enabled = effective_enabled(&entry, choice.as_ref(), platform.default_enabled);
            let controls = self.controls.get(&option.id);
            let id = option.id.clone();

            let toggle = match controls.and_then(|controls| controls.mode.clone()) {
                Some(mode) => h_flex()
                    .gap_2()
                    .child(div().w(px(130.0)).child(Select::new(&mode).small()))
                    .child(option.label.clone())
                    .into_any_element(),
                None => Checkbox::new(SharedString::from(format!("profile-jvm-{}", option.id)))
                    .label(option.label.clone())
                    .checked(enabled)
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |section, value: &bool, _, cx| {
                            let value = *value;
                            section.set_choice(&id, |choice| choice.enabled = Some(value), cx);
                        }
                    }))
                    .into_any_element(),
            };

            let mut row = h_flex().gap_2().items_center().child(toggle).child(
                Button::new(SharedString::from(format!("profile-jvm-help-{}", option.id)))
                    .label("?")
                    .ghost()
                    .small()
                    .tooltip(tooltip_text(&option)),
            );
            if let Some(number) = controls.and_then(|controls| controls.number.clone()) {
                row = row.child(div().w(px(110.0)).child(NumberInput::new(&number).small().disabled(!enabled)));
                if let (Some(min), Some(max)) = (option.min, option.max) {
                    row = row.child(div().text_sm().child(format!("{min}–{max}")));
                }
            }
            if let Some(values) = controls.and_then(|controls| controls.values.clone()) {
                row = row.child(div().w(px(160.0)).child(Select::new(&values).small().disabled(!enabled)));
            }
            let url = platform.evidence.url.clone();
            row = row.child(
                Button::new(SharedString::from(format!("profile-jvm-evidence-{}", option.id)))
                    .label(t::instance::profile_jvm::evidence())
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
            if choice.is_some() {
                row = row.child(
                    Button::new(SharedString::from(format!("profile-jvm-reset-{}", option.id)))
                        .label(t::instance::profile_jvm::reset())
                        .ghost()
                        .small()
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |section, _, window, cx| section.reset(&id, window, cx)
                        })),
                );
            }

            list = list.child(
                v_flex()
                    .gap_0p5()
                    .child(row)
                    .child(div().text_sm().pl_6().child(status_text(&entry, java_major))),
            );
        }
        list.into_any_element()
    }
}
