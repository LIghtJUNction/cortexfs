use super::{parse_boolean, parse_list, parse_number};
use crate::error::Result;

#[test]
fn parses_routing_keys_without_empty_entries() {
    assert_eq!(parse_list(" a, ,b "), vec!["a", "b"]);
}

#[test]
fn parses_bounded_configuration_values() -> Result<()> {
    assert_eq!(parse_number("prefetch", "4")?, 4);
    assert!(parse_number("prefetch", "x").is_err());
    assert!(parse_boolean("ack", "YES")?);
    assert!(!parse_boolean("ack", "off")?);
    assert!(parse_boolean("ack", "maybe").is_err());
    Ok(())
}
