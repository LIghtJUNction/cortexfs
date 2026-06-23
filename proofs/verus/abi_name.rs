use vstd::prelude::*;

verus! {

pub open spec fn max_object_name_len() -> nat {
    64nat
}

pub open spec fn is_ascii_digit(byte: u8) -> bool {
    48 <= byte && byte <= 57
}

pub open spec fn is_ascii_lower(byte: u8) -> bool {
    97 <= byte && byte <= 122
}

pub open spec fn is_ascii_upper(byte: u8) -> bool {
    65 <= byte && byte <= 90
}

pub open spec fn is_ascii_alphanumeric(byte: u8) -> bool {
    is_ascii_digit(byte) || is_ascii_lower(byte) || is_ascii_upper(byte)
}

pub open spec fn is_object_name_extra(byte: u8) -> bool {
    byte == 46 || byte == 95 || byte == 43 || byte == 45
}

pub open spec fn is_object_name_body_byte(byte: u8) -> bool {
    is_ascii_alphanumeric(byte) || is_object_name_extra(byte)
}

pub open spec fn sock_suffix() -> Seq<u8> {
    seq![46u8, 115u8, 111u8, 99u8, 107u8]
}

pub open spec fn dot_d_suffix() -> Seq<u8> {
    seq![46u8, 100u8]
}

pub open spec fn has_suffix(name: Seq<u8>, suffix: Seq<u8>) -> bool {
    exists|prefix: Seq<u8>| name == prefix + suffix
}

pub open spec fn is_valid_object_name(name: Seq<u8>) -> bool {
    0 < name.len()
        && name.len() <= max_object_name_len()
        && is_ascii_alphanumeric(name[0])
        && (forall|i: int| #![auto] 0 <= i < name.len() ==> is_object_name_body_byte(name[i]))
        && !has_suffix(name, sock_suffix())
        && !has_suffix(name, dot_d_suffix())
}

pub proof fn valid_object_name_is_non_empty(name: Seq<u8>)
    requires
        is_valid_object_name(name),
    ensures
        0 < name.len(),
{
}

pub proof fn valid_object_name_is_bounded(name: Seq<u8>)
    requires
        is_valid_object_name(name),
    ensures
        name.len() <= max_object_name_len(),
{
}

pub proof fn valid_object_name_has_alphanumeric_head(name: Seq<u8>)
    requires
        is_valid_object_name(name),
    ensures
        is_ascii_alphanumeric(name[0]),
{
}

pub proof fn valid_object_name_has_only_path_component_bytes(name: Seq<u8>)
    requires
        is_valid_object_name(name),
    ensures
        forall|i: int|
            0 <= i < name.len() ==> name[i] != 0 && name[i] != 10 && name[i] != 47,
{
    assert forall|i: int| 0 <= i < name.len() implies name[i] != 0 && name[i] != 10 && name[i] != 47 by {
        assert(is_object_name_body_byte(name[i]));
    }
}

pub proof fn valid_object_name_rejects_control_suffixes(name: Seq<u8>)
    requires
        is_valid_object_name(name),
    ensures
        !has_suffix(name, sock_suffix()),
        !has_suffix(name, dot_d_suffix()),
{
}

} // verus!
