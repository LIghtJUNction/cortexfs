#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use cortexfs_module::{
        CORTEX_MODULE_ABI, CortexModule, ModuleContext, ModuleError, ModuleFuture, ModuleKind,
        ModuleMetadata, ModuleRegistry, ModuleState,
    };

    #[derive(Debug)]
    struct MockModule {
        metadata: ModuleMetadata,
    }

    impl CortexModule for MockModule {
        fn metadata(&self) -> &ModuleMetadata {
            &self.metadata
        }

        fn init<'a>(&'a mut self, _context: &'a ModuleContext) -> ModuleFuture<'a> {
            ready()
        }

        fn start(&mut self) -> ModuleFuture<'_> {
            ready()
        }

        fn stop(&mut self) -> ModuleFuture<'_> {
            ready()
        }

        fn shutdown(&mut self) -> ModuleFuture<'_> {
            ready()
        }
    }

    #[derive(Debug)]
    struct FailingModule {
        metadata: ModuleMetadata,
    }

    impl CortexModule for FailingModule {
        fn metadata(&self) -> &ModuleMetadata {
            &self.metadata
        }

        fn init<'a>(&'a mut self, _context: &'a ModuleContext) -> ModuleFuture<'a> {
            Box::pin(async {
                Err(ModuleError::Failed {
                    code: "EINIT".to_owned(),
                    message: "fixture failure".to_owned(),
                })
            })
        }

        fn start(&mut self) -> ModuleFuture<'_> {
            ready()
        }

        fn stop(&mut self) -> ModuleFuture<'_> {
            ready()
        }

        fn shutdown(&mut self) -> ModuleFuture<'_> {
            ready()
        }
    }

    fn ready<'a>() -> ModuleFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn block<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn registry_runs_modules_through_lifecycle() {
        let mut registry = ModuleRegistry::default();
        let metadata = ModuleMetadata::new("channel.mock", "1.0.0", ModuleKind::Channel)
            .with_capability("text", "send and receive text");
        assert_eq!(metadata.capabilities.len(), 1);
        assert_eq!(CORTEX_MODULE_ABI, "cortexfs.module/v1");
        assert!(registry.register(Box::new(MockModule { metadata })).is_ok());
        assert_eq!(
            registry.state("channel.mock"),
            Some(ModuleState::Registered)
        );

        let context = ModuleContext {
            instance: "test".to_owned(),
        };
        assert!(block(registry.init_all(&context)).is_ok());
        assert_eq!(
            registry.state("channel.mock"),
            Some(ModuleState::Initialized)
        );
        assert!(block(registry.start_all()).is_ok());
        assert_eq!(registry.state("channel.mock"), Some(ModuleState::Running));
        assert!(block(registry.stop_all()).is_ok());
        assert!(block(registry.shutdown_all()).is_ok());
        assert_eq!(registry.state("channel.mock"), Some(ModuleState::Shutdown));
    }

    #[test]
    fn registry_rejects_duplicate_module_ids() {
        let mut registry = ModuleRegistry::default();
        let metadata = ModuleMetadata::new("tool.mock", "1.0.0", ModuleKind::Tool);
        assert!(
            registry
                .register(Box::new(MockModule {
                    metadata: metadata.clone(),
                }))
                .is_ok()
        );
        assert_eq!(
            registry.register(Box::new(MockModule { metadata })),
            Err(ModuleError::Duplicate("tool.mock".to_owned()))
        );
    }

    #[test]
    fn registry_rejects_invalid_metadata_and_exposes_valid_metadata() {
        let mut registry = ModuleRegistry::default();
        let invalid = ModuleMetadata::new("bad/id", "1.0.0", ModuleKind::Tool);
        assert_eq!(
            registry.register(Box::new(MockModule { metadata: invalid })),
            Err(ModuleError::InvalidMetadata)
        );
        let metadata = ModuleMetadata::new("tool.valid", "1.0.0", ModuleKind::Tool);
        assert!(registry.register(Box::new(MockModule { metadata })).is_ok());
        assert_eq!(
            registry
                .metadata("tool.valid")
                .map(|value| value.id.as_str()),
            Some("tool.valid")
        );
    }

    #[test]
    fn registry_wraps_lifecycle_failure_with_module_context() {
        let mut registry = ModuleRegistry::default();
        let metadata = ModuleMetadata::new("agent.failing", "1.0.0", ModuleKind::Agent);
        assert!(
            registry
                .register(Box::new(FailingModule { metadata }))
                .is_ok()
        );
        let context = ModuleContext {
            instance: "test".to_owned(),
        };
        let result = block(registry.init_all(&context));
        assert!(matches!(
            result,
            Err(ModuleError::Lifecycle {
                module,
                operation: "init",
                ..
            }) if module == "agent.failing"
        ));
        assert_eq!(
            registry.state("agent.failing"),
            Some(ModuleState::Registered)
        );
    }
}
