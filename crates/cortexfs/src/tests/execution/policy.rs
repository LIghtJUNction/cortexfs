#[test]
fn policy_v0_allows_only_exact_rules() {
    let parsed = PolicyV0::parse(
        "\
allow executor_t tool:fs.read execute
allow executor_t model:debug/echo use
allow executor_t shared:project-a read
",
    );
    let policy = ok!(parsed);

    assert!(policy.allows(
        "executor_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(policy.allows(
        "executor_t",
        PolicyObjectClass::Model,
        "debug/echo",
        PolicyPermission::Use
    ));
    assert!(!policy.allows(
        "executor_t",
        PolicyObjectClass::Tool,
        "shell.exec",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "reviewer_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "executor_t",
        PolicyObjectClass::Shared,
        "project-a",
        PolicyPermission::Write
    ));
}

#[test]
fn policy_v0_checks_child_authority_subset() {
    let parent = PolicyV0::parse(
        "\
allow executor_t tool:fs.read execute
allow executor_t model:debug/echo use
allow executor_t shared:project-a read
allow executor_t session:default resume
",
    );
    let parent = ok!(parent);

    let child = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t model:debug/echo use
allow reviewer_t shared:project-a read
",
    );
    let child = ok!(child);
    assert!(child.is_authority_subset_of(&parent, "reviewer_t", "executor_t"));
    assert!(!child.is_exact_subset_of(&parent));

    let expanded_tool = PolicyV0::parse(
        "\
allow reviewer_t tool:shell.exec execute
",
    );
    let expanded_tool = ok!(expanded_tool);
    assert!(!expanded_tool.is_authority_subset_of(&parent, "reviewer_t", "executor_t"));

    let wrong_subject = PolicyV0::parse(
        "\
allow other_t tool:fs.read execute
",
    );
    let wrong_subject = ok!(wrong_subject);
    assert!(!wrong_subject.is_authority_subset_of(&parent, "reviewer_t", "executor_t"));
}

#[test]
fn policy_v0_rejects_invalid_rules() {
    assert_eq!(
        PolicyRule::parse("deny executor_t tool:fs.read execute"),
        Err(PolicyError::ExpectedAllow)
    );
    assert_eq!(
        PolicyRule::parse("allow executor_t provider:openai use"),
        Err(PolicyError::UnknownClass)
    );
    assert_eq!(
        PolicyRule::parse("allow executor_t tool:fs.read use"),
        Err(PolicyError::UnknownPermission)
    );
    assert_eq!(
        PolicyRule::parse("allow executor_t tool:* execute"),
        Err(PolicyError::InvalidName)
    );
    assert_eq!(
        PolicyRule::parse("allow executor_t tool:fs.read execute extra"),
        Err(PolicyError::WrongFieldCount)
    );
}

#[test]
fn mount_table_parses_fixed_v0_format() {
    let parsed = MountTable::parse(
        "\
/ctx\t/ctx\tro\trbind,nosuid,nodev,noexec
/home/me/project\t/work\trw\trbind,nosuid,nodev
/tmp\t/tmp\trw\t-
",
    );
    let table = ok!(parsed);
    assert_eq!(table.entries().len(), 3);

    let Some(first) = table.entries().first() else {
        return;
    };
    assert_eq!(first.source(), "/ctx");
    assert_eq!(first.target(), "/ctx");
    assert_eq!(first.mode(), MountMode::ReadOnly);
    assert_eq!(
        first.options(),
        [
            MountOption::RecursiveBind,
            MountOption::NoSuid,
            MountOption::NoDev,
            MountOption::NoExec
        ]
    );

    let Some(last) = table.entries().last() else {
        return;
    };
    assert!(last.options().is_empty());
}

#[test]
fn mount_table_checks_child_attenuation() {
    let parent = MountTable::parse(
        "\
/home/me/project\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    let parent = ok!(parent);

    let narrowed = MountTable::parse(
        "\
/home/me/project\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    let narrowed = ok!(narrowed);
    assert!(narrowed.is_subset_of(&parent));

    let write_expansion = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\trw\tbind,nosuid,nodev,noexec
",
    );
    let write_expansion = ok!(write_expansion);
    assert!(!write_expansion.is_subset_of(&parent));

    let removed_safety = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev
",
    );
    let removed_safety = ok!(removed_safety);
    assert!(!removed_safety.is_subset_of(&parent));

    let hidden_parent_path = MountTable::parse(
        "\
/secret\t/secret\tro\tbind,nosuid,nodev,noexec
",
    );
    let hidden_parent_path = ok!(hidden_parent_path);
    assert!(!hidden_parent_path.is_subset_of(&parent));
}

#[test]
fn mount_table_rejects_invalid_v0_format() {
    assert_eq!(
        MountEntry::parse("ctx\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\tctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx/shared/project-a/..\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx/./shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\0/shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\u{1b}/shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tbad\trbind"),
        Err(MountError::InvalidMode)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tbind,rbind"),
        Err(MountError::ConflictingBindOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\trbind,rbind"),
        Err(MountError::DuplicateOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tdev"),
        Err(MountError::InvalidOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro"),
        Err(MountError::WrongFieldCount)
    );
}
use super::*;
