use cortexfs_protocol::{WireProtocol, transcode_request};
use std::io::Write;
use std::time::Instant;

const CHAT: &[u8] = br#"{"model":"bench","messages":[{"role":"user","content":"measure"}]}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        ("direct", WireProtocol::OpenAiChat, WireProtocol::Gemini),
        ("via-ir", WireProtocol::OpenAiChat, WireProtocol::Anthropic),
    ];
    let iterations = 20_000_u32;
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    for (name, source, target) in cases {
        let started = Instant::now();
        let mut bytes = 0_usize;
        for _ in 0..iterations {
            let result = transcode_request(source, target, CHAT)?;
            bytes = std::hint::black_box(bytes.saturating_add(result.bytes.len()));
        }
        let elapsed = started.elapsed();
        let seconds = elapsed.as_secs_f64();
        writeln!(
            output,
            "{name} iterations={iterations} elapsed_ms={:.3} requests_per_sec={:.0} bytes={bytes}",
            seconds * 1_000.0,
            f64::from(iterations) / seconds
        )?;
    }
    Ok(())
}
