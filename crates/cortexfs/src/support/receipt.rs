use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Result};

/// Reads system entropy and returns exactly `N * 2` lowercase hexadecimal bytes.
pub(crate) fn random_hex<const N: usize>() -> Result<String> {
    let mut bytes = [0_u8; N];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(N * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").map_err(std::io::Error::other)?;
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::random_hex;

    #[test]
    fn random_hex_has_exact_lowercase_format() -> Result<(), Box<dyn std::error::Error>> {
        let token = random_hex::<32>()?;
        assert_eq!(token.len(), 64);
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        Ok(())
    }
}
