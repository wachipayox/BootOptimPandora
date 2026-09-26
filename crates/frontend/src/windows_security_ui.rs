use command::DefenderProcessResult;
use gpui::{App, SharedString, Window};
use gpui_component::{
    WindowExt,
    notification::{Notification, NotificationType},
};

pub(crate) fn push_result(window: &mut Window, cx: &mut App, result: DefenderProcessResult) {
    let (kind, message) = match result {
        DefenderProcessResult::Enabled => {
            (NotificationType::Success, "Defender optimization enabled for this Wachiland Launcher executable.")
        },
        DefenderProcessResult::Removed => {
            (NotificationType::Success, "Wachiland Launcher's Defender process exclusion was removed.")
        },
        DefenderProcessResult::AlreadyEnabledByPandora => {
            (NotificationType::Info, "Wachiland Launcher already controls this Defender process exclusion.")
        },
        DefenderProcessResult::PresentButNotOwned => (
            NotificationType::Info,
            "The same Defender exclusion already exists. Wachiland Launcher did not claim or change it.",
        ),
        DefenderProcessResult::PreviousEntryAlreadyAbsent => (
            NotificationType::Info,
            "The recorded Wachiland Launcher exclusion was already absent. Its ownership record was cleared.",
        ),
        DefenderProcessResult::NothingOwned => {
            (NotificationType::Info, "Wachiland Launcher has no owned Defender process exclusion to remove.")
        },
        DefenderProcessResult::InvalidOwnershipRecord => (
            NotificationType::Warning,
            "Wachiland Launcher refused the change because its exclusion ownership record was not valid for this installation.",
        ),
        DefenderProcessResult::UacDenied => (
            NotificationType::Info,
            "Administrator approval was declined. Nothing changed and Wachiland Launcher can continue normally.",
        ),
        DefenderProcessResult::DefenderBlockedOrUnavailable => (
            NotificationType::Warning,
            "Microsoft Defender or device policy did not allow the change. Wachiland Launcher can continue normally.",
        ),
        DefenderProcessResult::Failed => (
            NotificationType::Error,
            "The Defender operation could not be verified. Wachiland Launcher left launch behavior unchanged.",
        ),
    };

    let mut notification: Notification = (kind, SharedString::new_static(message)).into();
    if matches!(result, DefenderProcessResult::Failed) {
        notification = notification.autohide(false);
    }
    window.push_notification(notification, cx);
}
