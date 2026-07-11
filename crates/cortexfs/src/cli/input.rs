use std::io::{self, Read};

pub fn read_limited_input_text(
    reader: impl Read,
    max_bytes: usize,
    too_large_message: &'static str,
) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut input = String::new();
    reader.take(limit).read_to_string(&mut input)?;
    if input.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            too_large_message,
        ));
    }
    Ok(input)
}
