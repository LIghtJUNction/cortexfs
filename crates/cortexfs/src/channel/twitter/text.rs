pub(super) fn chunks(text: &str, max_bytes: usize) -> Vec<String> {
    if max_bytes == 0 {
        return vec![text.to_owned()];
    }
    let mut chunks = Vec::new();
    let mut remaining = text.trim();
    while !remaining.is_empty() {
        if remaining.len() <= max_bytes {
            chunks.push(remaining.to_owned());
            break;
        }
        let end = remaining
            .char_indices()
            .take_while(|&(index, _)| index <= max_bytes)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(0);
        let prefix = remaining.get(..end).unwrap_or(remaining);
        let split = prefix.rfind(' ').unwrap_or(prefix.len());
        let (head, tail) = remaining.split_at(split);
        chunks.push(head.to_owned());
        remaining = tail.trim_start();
    }
    chunks
}
