#[cfg(test)]
mod support {
    #[expect(
        clippy::redundant_pub_crate,
        reason = "the sibling test module uses this private fixture"
    )]
    pub(super) mod http;
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::process::Command;

    use serde_json::json;

    use super::support::http;

    #[test]
    fn cloud_request_uses_registry_input_map() -> Result<(), Box<dyn std::error::Error>> {
        let (address, receiver, server) = http::start()?;
        let path = std::env::temp_dir().join(format!(
            "cortexfs-futureagi-evaluate-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{"schema_version":"ATIF-v1.7","agent":{"name":"cortexfs","version":"0.1.21"},"steps":[{"step_id":1,"source":"user","message":"Inspect"},{"step_id":2,"source":"agent","message":"Healthy"}]}"#,
        )?;
        let binary = std::env::var("CARGO_BIN_EXE_cortexfs-futureagi")?;
        let output = Command::new(binary)
            .args(["evaluate", "--trajectory"])
            .arg(&path)
            .args(["--eval", "answer_relevancy", "--base-url"])
            .arg(format!("http://{address}"))
            .env("FI_API_KEY", "test-api")
            .env("FI_SECRET_KEY", "test-secret")
            .output()?;
        fs::remove_file(&path)?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let request = receiver.recv()?;
        let inputs = request
            .get("inputs")
            .ok_or_else(|| io::Error::other("request omitted inputs"))?;
        assert_eq!(inputs.get("input"), Some(&json!(["Inspect"])));
        assert_eq!(inputs.get("output"), Some(&json!(["Healthy"])));
        assert!(inputs.get("context").is_none());
        server
            .join()
            .map_err(|error| io::Error::other(format!("mock server panicked: {error:?}")))??;
        Ok(())
    }
}
