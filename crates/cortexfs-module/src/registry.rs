use std::collections::BTreeMap;

use crate::{CortexModule, ModuleContext, ModuleError, ModuleResult, ModuleState};

/// Deterministic static module registry used by the runtime host.
#[derive(Default)]
pub struct ModuleRegistry {
    modules: BTreeMap<String, (Box<dyn CortexModule>, ModuleState)>,
}

impl std::fmt::Debug for ModuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleRegistry")
            .field("modules", &self.modules.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ModuleRegistry {
    /// Registers one module after checking its ABI metadata.
    pub fn register(&mut self, module: Box<dyn CortexModule>) -> ModuleResult<()> {
        let metadata = module.metadata();
        if !metadata.is_valid() {
            return Err(ModuleError::InvalidMetadata);
        }
        if self.modules.contains_key(&metadata.id) {
            return Err(ModuleError::Duplicate(metadata.id.clone()));
        }
        self.modules
            .insert(metadata.id.clone(), (module, ModuleState::Registered));
        Ok(())
    }

    /// Initializes all modules in stable id order.
    pub async fn init_all(&mut self, context: &ModuleContext) -> ModuleResult<()> {
        for (id, value) in &mut self.modules {
            let module = &mut value.0;
            let state = &mut value.1;
            if *state != ModuleState::Registered {
                return Err(ModuleError::InvalidState);
            }
            lifecycle(id, "init", module.init(context).await)?;
            *state = ModuleState::Initialized;
        }
        Ok(())
    }

    /// Starts all initialized modules in stable id order.
    pub async fn start_all(&mut self) -> ModuleResult<()> {
        for (id, value) in &mut self.modules {
            let module = &mut value.0;
            let state = &mut value.1;
            if *state != ModuleState::Initialized {
                return Err(ModuleError::InvalidState);
            }
            lifecycle(id, "start", module.start().await)?;
            *state = ModuleState::Running;
        }
        Ok(())
    }

    /// Stops all running modules in stable id order.
    pub async fn stop_all(&mut self) -> ModuleResult<()> {
        for (id, value) in &mut self.modules {
            let module = &mut value.0;
            let state = &mut value.1;
            if *state != ModuleState::Running {
                return Err(ModuleError::InvalidState);
            }
            lifecycle(id, "stop", module.stop().await)?;
            *state = ModuleState::Stopped;
        }
        Ok(())
    }

    /// Shuts down all stopped modules in stable id order.
    pub async fn shutdown_all(&mut self) -> ModuleResult<()> {
        for (id, value) in &mut self.modules {
            let module = &mut value.0;
            let state = &mut value.1;
            if *state != ModuleState::Stopped {
                return Err(ModuleError::InvalidState);
            }
            lifecycle(id, "shutdown", module.shutdown().await)?;
            *state = ModuleState::Shutdown;
        }
        Ok(())
    }

    /// Returns registered module ids in deterministic order.
    #[must_use = "iterate over the registered module ids"]
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    /// Returns the current registry state for one module.
    #[must_use]
    pub fn state(&self, id: &str) -> Option<ModuleState> {
        self.modules.get(id).map(|value| value.1)
    }

    /// Returns immutable metadata for one registered module.
    #[must_use]
    pub fn metadata(&self, id: &str) -> Option<&crate::ModuleMetadata> {
        self.modules.get(id).map(|value| value.0.metadata())
    }
}

fn lifecycle(id: &str, operation: &'static str, result: ModuleResult<()>) -> ModuleResult<()> {
    result.map_err(|source| ModuleError::Lifecycle {
        module: id.to_owned(),
        operation,
        source: Box::new(source),
    })
}
