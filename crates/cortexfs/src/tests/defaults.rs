#[test]
fn worker_role_names_default_to_spark_model() {
    for name in ["worker", "worker-fast", "executor", "executor-fast"] {
        assert!(is_worker_agent_name(name), "{name}");
        assert_eq!(default_agent_model_for_name(name), DEFAULT_WORKER_MODEL);
    }
}

#[test]
fn dedicated_worker_role_names_exclude_shared_entries() {
    for name in ["worker", "executor"] {
        assert!(!is_dedicated_worker_agent_name(name), "{name}");
    }
    for name in ["worker-fast", "executor-fast"] {
        assert!(is_dedicated_worker_agent_name(name), "{name}");
    }
}

#[test]
fn non_worker_role_names_keep_main_default() {
    for name in ["coder", "reviewer", "work", "task-worker"] {
        assert!(!is_worker_agent_name(name), "{name}");
        assert_eq!(default_agent_model_for_name(name), "main");
    }
}

#[test]
fn bootstrap_model_driver_is_supported_by_runner() {
    assert_eq!(
        crate::object::bootstrap::default_model_control_value("fixture", "driver"),
        "default=openai-chat"
    );
}
use super::*;
