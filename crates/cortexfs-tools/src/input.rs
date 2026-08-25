use std::io::{self, Read};

pub fn read_text_from_stdin_limited(reader: impl Read, max_bytes: usize) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut content = String::new();
    reader.take(limit).read_to_string(&mut content)?;
    if content.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stdin exceeds fs.write input limit",
        ));
    }
    Ok(content)
}
