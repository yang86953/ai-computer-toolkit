//! Linux 组合根装配的领域 Module。

#[path = "modules/build_info.rs"]
pub(crate) mod build_info;

#[path = "modules/linux_application_discovery.rs"]
pub(crate) mod application_discovery;

#[path = "modules/linux_application_session_discovery.rs"]
pub(crate) mod application_session_discovery;

#[path = "modules/linux_capability_assessment.rs"]
pub(crate) mod capability_assessment;

#[path = "modules/linux_capability_metadata.rs"]
pub(crate) mod capability_metadata;

#[path = "modules/linux_portal_screenshot.rs"]
pub(crate) mod portal_screenshot;

#[path = "modules/desktop_session.rs"]
pub(crate) mod desktop_session;

#[path = "modules/linux_process_termination.rs"]
pub(crate) mod process_termination;

#[path = "modules/linux_application_launch.rs"]
pub(crate) mod application_launch;

#[path = "modules/linux_uix_window.rs"]
pub(crate) mod uix_window;

#[path = "modules/linux_uix_closed_wait.rs"]
pub(crate) mod uix_closed_wait;

#[path = "modules/linux_uix_close.rs"]
pub(crate) mod uix_close;

#[path = "modules/linux_uix_window_close_transition.rs"]
pub(crate) mod uix_window_close_transition;

#[path = "modules/linux_uix_action.rs"]
pub(crate) mod uix_action;

#[path = "modules/linux_uix_element_location.rs"]
pub(crate) mod uix_element_location;

#[path = "modules/linux_uix_element_wait.rs"]
pub(crate) mod uix_element_wait;

#[path = "modules/linux_uix_element_transition.rs"]
pub(crate) mod uix_element_transition;

#[path = "modules/linux_uix_key_input.rs"]
pub(crate) mod uix_key_input;

#[path = "modules/linux_uix_key_sequence.rs"]
pub(crate) mod uix_key_sequence;

#[path = "modules/linux_uix_key_sequence_transition.rs"]
pub(crate) mod uix_key_sequence_transition;

#[path = "modules/linux_uix_key_transition.rs"]
pub(crate) mod uix_key_transition;

#[path = "modules/linux_uix_input_sequence.rs"]
pub(crate) mod uix_input_sequence;

#[path = "modules/linux_uix_input_sequence_transition.rs"]
pub(crate) mod uix_input_sequence_transition;

#[path = "modules/linux_uix_pointer_input.rs"]
pub(crate) mod uix_pointer_input;

#[path = "modules/linux_uix_pointer_click_sequence.rs"]
pub(crate) mod uix_pointer_click_sequence;

#[path = "modules/linux_uix_pointer_click_sequence_transition.rs"]
pub(crate) mod uix_pointer_click_sequence_transition;

#[path = "modules/linux_uix_pointer_click_transition.rs"]
pub(crate) mod uix_pointer_click_transition;

#[path = "modules/linux_uix_pointer_move_transition.rs"]
pub(crate) mod uix_pointer_move_transition;

#[path = "modules/linux_uix_pointer_sequence.rs"]
pub(crate) mod uix_pointer_sequence;

#[path = "modules/linux_uix_pointer_sequence_transition.rs"]
pub(crate) mod uix_pointer_sequence_transition;

#[path = "modules/linux_uix_pointer_move_sequence.rs"]
pub(crate) mod uix_pointer_move_sequence;

#[path = "modules/linux_uix_pointer_move_sequence_transition.rs"]
pub(crate) mod uix_pointer_move_sequence_transition;

#[path = "modules/linux_uix_state_wait.rs"]
pub(crate) mod uix_state_wait;

#[path = "modules/linux_uix_lifecycle_transition.rs"]
pub(crate) mod uix_lifecycle_transition;

#[path = "modules/linux_uix_pointer_drag.rs"]
pub(crate) mod uix_pointer_drag;

#[path = "modules/linux_uix_pointer_drag_transition.rs"]
pub(crate) mod uix_pointer_drag_transition;

#[path = "modules/linux_uix_lifecycle.rs"]
pub(crate) mod uix_lifecycle;

#[path = "modules/linux_uix_lifecycle_sequence.rs"]
pub(crate) mod uix_lifecycle_sequence;

#[path = "modules/linux_uix_window_lifecycle_sequence_transition.rs"]
pub(crate) mod uix_lifecycle_sequence_transition;

#[path = "modules/linux_uix_window_screenshot.rs"]
pub(crate) mod uix_window_screenshot;

#[path = "modules/linux_uix_window_activation.rs"]
pub(crate) mod uix_window_activation;

#[path = "modules/linux_uix_window_activation_transition.rs"]
pub(crate) mod uix_window_activation_transition;

#[cfg(feature = "linux-atspi-candidate")]
#[path = "modules/linux_window_observation.rs"]
pub(crate) mod window_observation;

#[cfg(feature = "linux-atspi-candidate")]
#[path = "modules/linux_accessibility.rs"]
pub(crate) mod accessibility;

#[path = "modules/linux_media_session.rs"]
pub(crate) mod media_session;

#[path = "modules/linux_media_playback.rs"]
pub(crate) mod media_playback;
