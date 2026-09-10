//! Linux 组合根当前需要的窄、无平台类型 Component 集合。

#[path = "components/opaque_id.rs"]
#[allow(dead_code)]
pub(crate) mod opaque_id;

#[path = "components/uix_semantic_action_contract.rs"]
pub(crate) mod uix_semantic_action_contract;

#[path = "components/uix_semantic_identity.rs"]
pub(crate) mod uix_semantic_identity;

#[path = "components/uix_element_location_contract.rs"]
pub(crate) mod uix_element_location_contract;

#[path = "components/uix_element_wait_contract.rs"]
pub(crate) mod uix_element_wait_contract;

#[path = "components/uix_element_transition_contract.rs"]
pub(crate) mod uix_element_transition_contract;

#[path = "components/uix_key_input_contract.rs"]
pub(crate) mod uix_key_input_contract;

#[path = "components/uix_key_transition_contract.rs"]
pub(crate) mod uix_key_transition_contract;

#[path = "components/uix_key_sequence_contract.rs"]
pub(crate) mod uix_key_sequence_contract;

#[path = "components/uix_key_sequence_transition_contract.rs"]
pub(crate) mod uix_key_sequence_transition_contract;

#[path = "components/uix_input_sequence_contract.rs"]
pub(crate) mod uix_input_sequence_contract;

#[path = "components/uix_input_sequence_transition_contract.rs"]
pub(crate) mod uix_input_sequence_transition_contract;

#[path = "components/uix_pointer_input_contract.rs"]
pub(crate) mod uix_pointer_input_contract;

#[path = "components/uix_pointer_click_sequence_contract.rs"]
pub(crate) mod uix_pointer_click_sequence_contract;

#[path = "components/uix_pointer_click_sequence_transition_contract.rs"]
pub(crate) mod uix_pointer_click_sequence_transition_contract;

#[path = "components/uix_pointer_click_transition_contract.rs"]
pub(crate) mod uix_pointer_click_transition_contract;

#[path = "components/uix_pointer_move_transition_contract.rs"]
pub(crate) mod uix_pointer_move_transition_contract;

#[path = "components/uix_pointer_sequence_contract.rs"]
pub(crate) mod uix_pointer_sequence_contract;

#[path = "components/uix_pointer_sequence_transition_contract.rs"]
pub(crate) mod uix_pointer_sequence_transition_contract;

#[path = "components/uix_pointer_move_sequence_contract.rs"]
pub(crate) mod uix_pointer_move_sequence_contract;

#[path = "components/uix_pointer_move_sequence_transition_contract.rs"]
pub(crate) mod uix_pointer_move_sequence_transition_contract;

#[path = "components/uix_pointer_drag_contract.rs"]
pub(crate) mod uix_pointer_drag_contract;

#[path = "components/uix_pointer_drag_transition_contract.rs"]
pub(crate) mod uix_pointer_drag_transition_contract;

#[path = "components/uix_window_lifecycle_contract.rs"]
pub(crate) mod uix_window_lifecycle_contract;

#[path = "components/uix_window_lifecycle_sequence_contract.rs"]
pub(crate) mod uix_window_lifecycle_sequence_contract;

#[path = "components/uix_window_lifecycle_sequence_transition_contract.rs"]
pub(crate) mod uix_window_lifecycle_sequence_transition_contract;

#[path = "components/window_revision_wait_contract.rs"]
pub(crate) mod window_revision_wait_contract;

#[path = "components/uix_window_closed_wait_contract.rs"]
pub(crate) mod uix_window_closed_wait_contract;

#[path = "components/uix_window_state_wait_contract.rs"]
pub(crate) mod uix_window_state_wait_contract;

#[path = "components/uix_window_lifecycle_transition_contract.rs"]
pub(crate) mod uix_window_lifecycle_transition_contract;

#[path = "components/uix_window_close_contract.rs"]
pub(crate) mod uix_window_close_contract;

#[path = "components/uix_window_close_transition_contract.rs"]
pub(crate) mod uix_window_close_transition_contract;

#[path = "components/uix_window_screenshot_contract.rs"]
pub(crate) mod uix_window_screenshot_contract;

#[path = "components/uix_window_activation_contract.rs"]
pub(crate) mod uix_window_activation_contract;

#[path = "components/uix_window_activation_transition_contract.rs"]
pub(crate) mod uix_window_activation_transition_contract;

#[path = "components/linux_media_worker_contract_common.rs"]
pub(crate) mod linux_media_worker_contract_common;

#[path = "components/linux_media_observation_worker_contract.rs"]
pub(crate) mod linux_media_observation_worker_contract;

#[path = "components/linux_media_control_worker_contract.rs"]
pub(crate) mod linux_media_control_worker_contract;

#[path = "components/linux_media_worker_process.rs"]
pub(crate) mod linux_media_worker_process;

#[path = "components/linux_user_bus_endpoint.rs"]
pub(crate) mod linux_user_bus_endpoint;

#[path = "components/media_playback_contract.rs"]
pub(crate) mod media_playback_contract;

#[path = "components/uix_screenshot_payload.rs"]
pub(crate) mod uix_screenshot_payload;

#[path = "components/atomic_file_linux.rs"]
pub(crate) mod atomic_file;

#[path = "components/bounded_json_input.rs"]
pub(crate) mod bounded_json_input;

#[path = "components/cancellation_linux.rs"]
pub(crate) mod cancellation;

#[path = "components/json_postcondition.rs"]
pub(crate) mod json_postcondition;

#[path = "components/sequence_result_budget.rs"]
pub(crate) mod sequence_result_budget;

#[path = "components/sequence_execution_budget.rs"]
pub(crate) mod sequence_execution_budget;

#[path = "components/sequence_step_protocol.rs"]
pub(crate) mod sequence_step_protocol;

#[path = "components/secure_nonce_linux.rs"]
pub(crate) mod secure_nonce_linux;

#[path = "components/linux_sequence_worker.rs"]
pub(crate) mod linux_sequence_worker;

#[path = "components/linux_host_identity.rs"]
pub(crate) mod linux_host_identity;

#[path = "components/desktop_session_identity.rs"]
pub(crate) mod desktop_session_identity;

#[path = "components/desktop_interaction.rs"]
pub(crate) mod desktop_interaction;

#[path = "components/keyboard_input_contract.rs"]
pub(crate) mod keyboard_input_contract;

#[path = "components/desktop_session_keyboard_input.rs"]
pub(crate) mod desktop_session_keyboard_input;

#[path = "components/desktop_session_input_cancellation.rs"]
pub(crate) mod desktop_session_input_cancellation;

#[path = "components/desktop_session_input_liveness.rs"]
pub(crate) mod desktop_session_input_liveness;

#[path = "components/desktop_frame_stream.rs"]
pub(crate) mod desktop_frame_stream;
#[path = "components/desktop_frame_changes.rs"]
pub(crate) mod desktop_frame_changes;
#[path = "components/desktop_session_frame_capture.rs"]
pub(crate) mod desktop_session_frame_capture;

#[path = "components/linux_process_termination.rs"]
pub(crate) mod linux_process_termination;

#[path = "components/linux_application_open.rs"]
pub(crate) mod linux_application_open;

#[path = "components/byte_digest.rs"]
pub(crate) mod byte_digest;

#[path = "components/pointer_input_contract.rs"]
#[allow(dead_code)]
pub(crate) mod pointer_input_contract;

#[path = "components/desktop_session_pointer_input.rs"]
pub(crate) mod desktop_session_pointer_input;

#[path = "components/output_guard.rs"]
// Linux 首个纵切只用单文件门禁；目录门禁保留给后续平台 Module。
#[allow(dead_code)]
pub(crate) mod output_guard;
