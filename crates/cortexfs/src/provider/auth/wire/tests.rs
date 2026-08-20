use super::*;

#[test]
fn request_rejects_a_wrong_abi_without_echoing_the_key() {
    let result = AuthWireFrame::<AuthWireRequest>::decode(
        r#"{"abi":"wrong","frame":{"type":"api_key","request_id":"r","provider":"p","profile":"q","key":"secret"}}"#,
    );
    assert!(matches!(result, Err(AuthWireError::Abi)));
}
