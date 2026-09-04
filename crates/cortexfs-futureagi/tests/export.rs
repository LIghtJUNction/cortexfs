use std::fs;
use std::process::Command;

use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_maps_atif_dialogue_to_futureagi_case() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "cortexfs-futureagi-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            r#"{
          "schema_version":"ATIF-v1.7",
          "agent":{"name":"cortexfs","version":"0.1.21"},
          "steps":[
            {"step_id":1,"source":"user","message":"Inspect this run."},
            {"step_id":2,"source":"agent","message":"The run is healthy.","observation":{"results":[{"content":"ok"}]}}
          ]
        }"#,
        )?;
        let binary = std::env::var("CARGO_BIN_EXE_cortexfs-futureagi")?;
        let output = Command::new(binary)
            .args(["export", "--trajectory"])
            .arg(&path)
            .arg("--include-context")
            .output()?;
        fs::remove_file(&path)?;
        assert!(output.status.success());
        let cases: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        let case = cases
            .first()
            .ok_or_else(|| std::io::Error::other("export returned no cases"))?;
        assert_eq!(case["input"], "Inspect this run.");
        assert_eq!(case["output"], "The run is healthy.");
        assert_eq!(case["context"], "ok");
        Ok(())
    }
}
