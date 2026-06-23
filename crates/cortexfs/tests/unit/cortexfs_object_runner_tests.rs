use super::{run, ObjectPath};
use std::ffi::OsString;
use std::path::Path;

#[test]
fn object_path_parses_class_and_name() {
    assert_eq!(
        ObjectPath::parse(Path::new("/ctx/model/debug/echo")),
        Ok(ObjectPath {
            class: "model".to_owned(),
            name: "debug/echo".to_owned(),
        })
    );
}

#[test]
fn runner_rejects_missing_object_path() {
    assert_eq!(run(Vec::new()), Err("missing object path".to_owned()));
}

#[test]
fn runner_rejects_unknown_model() {
    assert_eq!(
        run(vec![OsString::from("/ctx/model/openai/gpt-4o")]),
        Err("model openai/gpt-4o is not handled by this runner".to_owned())
    );
}
