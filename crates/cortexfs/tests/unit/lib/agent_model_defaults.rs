#[test]
fn worker_role_names_default_to_spark_model() {
    for name in ["worker", "worker-fast", "executor", "executor-fast"] {
        assert!(is_worker_agent_name(name), "{name}");
        assert_eq!(default_agent_model_for_name(name), DEFAULT_WORKER_MODEL);
    }
}

#[test]
fn non_worker_role_names_keep_main_default() {
    for name in ["coder", "reviewer", "work", "task-worker"] {
        assert!(!is_worker_agent_name(name), "{name}");
        assert_eq!(default_agent_model_for_name(name), "main");
    }
}
