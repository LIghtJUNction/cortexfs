#[cfg(test)]
mod tests {
    use cortexfs_module::{
        CORTEX_MODULE_WIRE_ABI, MAX_MODULE_FRAME_BYTES, ModuleFrame, ModuleMetadata,
        ModuleWireError,
    };
    use serde_json::json;

    #[test]
    fn wire_frame_round_trips_without_rust_layout_assumptions() {
        let frame = ModuleFrame::Call {
            request_id: "call-1".to_owned(),
            method: "channel.send".to_owned(),
            payload: json!({"text": "hello"}),
        };
        let encoded = frame.encode();
        assert!(encoded.is_ok());
        let Ok(encoded) = encoded else { return };
        let decoded = ModuleFrame::decode(&encoded);
        assert!(decoded.is_ok());
        let Ok(decoded) = decoded else { return };
        assert_eq!(decoded, frame);
        assert_eq!(encoded.last(), Some(&b'\n'));
    }

    #[test]
    fn handshake_requires_wire_abi_and_valid_metadata() {
        let hello = ModuleFrame::Hello {
            abi: CORTEX_MODULE_WIRE_ABI.to_owned(),
            instance: "worker-1".to_owned(),
        };
        assert!(hello.encode().is_ok());
        let bad = ModuleFrame::Hello {
            abi: "cortexfs.module/v1".to_owned(),
            instance: "worker-1".to_owned(),
        };
        assert!(matches!(
            bad.encode(),
            Err(ModuleWireError::InvalidField("abi"))
        ));
        let ready = ModuleFrame::Ready {
            metadata: ModuleMetadata::new(
                "channel.example",
                "1.0.0",
                cortexfs_module::ModuleKind::Channel,
            ),
        };
        assert!(ready.encode().is_ok());
    }

    #[test]
    fn decoder_rejects_multiple_frames_and_oversized_payloads() {
        let hello = ModuleFrame::Hello {
            abi: CORTEX_MODULE_WIRE_ABI.to_owned(),
            instance: "worker-1".to_owned(),
        };
        let encoded = hello.encode();
        assert!(encoded.is_ok());
        let Ok(encoded) = encoded else { return };
        let mut two = encoded.clone();
        two.extend_from_slice(&encoded);
        assert!(matches!(
            ModuleFrame::decode(&two),
            Err(ModuleWireError::InvalidFraming)
        ));
        let huge = ModuleFrame::Error {
            request_id: None,
            code: "E2BIG".to_owned(),
            message: "x".repeat(MAX_MODULE_FRAME_BYTES),
        };
        assert!(matches!(huge.encode(), Err(ModuleWireError::FrameTooLarge)));
    }
}
