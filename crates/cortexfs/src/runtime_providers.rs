use fuse3::Inode;

use crate::runtime_types::ApiRoute;
use crate::validation::{
    allowed_provider_lines, normalize_allowed_providers, validate_control_write,
};
use crate::{
    API_FORMATS, EMPTY_TEXT, LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT, Node, PROVIDER_SPECS,
    RuntimeState, configured_provider_ids, default_provider_id, newline_list, provider_model_id,
    provider_spec, provider_supports_format, secret_rotating_id,
};

impl RuntimeState {
    pub fn write_user_allowed_providers(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let providers = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let value = normalize_allowed_providers(providers)?;
        if let Some(default_provider) = self.current_user_default_provider()
            && !allowed_provider_lines(&value).any(|provider| provider == default_provider)
        {
            return Err(libc::EINVAL.into());
        }
        if let Some(inode) = self.user_allowed_providers_inode {
            self.update_dynamic_file(inode, value);
        }
        self.refresh_user_model_access();
        self.refresh_user_routes();
        self.append_audit("home.1000.policy", "allowed_providers", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_user_default_provider(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let provider = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let trimmed = provider.trim();
        let provider = if trimmed.is_empty() {
            default_provider_id()
        } else if provider_spec(trimmed).is_some() {
            trimmed
        } else {
            return Err(libc::EINVAL.into());
        };
        if !self.is_provider_allowed(provider) {
            return Err(libc::EACCES.into());
        }
        if let Some(inode) = self.user_default_provider_inode {
            self.update_dynamic_file(inode, format!("{provider}\n"));
        }
        self.refresh_user_routes();
        self.append_audit("home.1000.route", "default_provider", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    pub fn write_user_models_refresh(&mut self, offset: u64, data: &[u8]) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        self.refresh_user_model_access();
        self.refresh_user_routes();
        if let Some(inode) = self.user_models_refresh_inode {
            self.update_dynamic_file(inode, "1\n");
        }
        self.update_dynamic_file(
            self.last_control_inode,
            LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT,
        );
        self.append_audit("home.1000.model", "refresh", "refreshed");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn is_provider_allowed(&self, provider: &str) -> bool {
        self.user_allowed_providers_content()
            .is_some_and(|content| {
                allowed_provider_lines(content).any(|allowed| allowed == provider)
            })
    }

    fn current_user_default_provider(&self) -> Option<&str> {
        let content = self
            .user_default_provider_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)?;
        Some(content.trim())
    }

    fn user_allowed_providers_content(&self) -> Option<&str> {
        self.user_allowed_providers_inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
    }

    pub fn write_provider_config(
        &mut self,
        inode: Inode,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<Option<u32>> {
        if let Some(provider) = self.provider_for_url_current_inode(inode) {
            return self
                .write_provider_url_current(provider, offset, data)
                .map(Some);
        }
        if let Some(provider) = self.provider_for_enabled_current_inode(inode) {
            return self
                .write_provider_enabled_current(provider, offset, data)
                .map(Some);
        }
        if let Some(provider) = self.provider_for_health_check_inode(inode) {
            return self
                .write_provider_health_check(provider, offset, data)
                .map(Some);
        }
        if let Some(provider) = self.provider_for_secret_rotate_inode(inode) {
            return self
                .write_provider_secret_rotate(provider, offset, data)
                .map(Some);
        }
        if let Some(provider) = self.provider_for_models_refresh_inode(inode) {
            return self
                .write_provider_models_refresh(provider, offset, data)
                .map(Some);
        }
        Ok(None)
    }

    fn write_provider_url_current(
        &mut self,
        provider: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let url = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let trimmed = url.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with("http://")
            && !trimmed.starts_with("https://")
        {
            return Err(libc::EINVAL.into());
        }
        let Some(inodes) = self.provider_url.get(provider).copied() else {
            return Err(libc::EINVAL.into());
        };
        let current = if trimmed.is_empty() {
            provider_spec(provider)
                .map_or(EMPTY_TEXT, |spec| spec.default_base_url)
                .to_owned()
        } else {
            format!("{trimmed}\n")
        };
        let source = if trimmed.is_empty() {
            "default\n"
        } else {
            "current\n"
        };
        if let Some(inode) = inodes.current {
            self.update_dynamic_file(inode, current.as_str());
        }
        if let Some(inode) = inodes.effective {
            self.update_dynamic_file(inode, current);
        }
        if let Some(inode) = inodes.source {
            self.update_dynamic_file(inode, source);
        }
        let audit_format = format!("provider.{provider}.url");
        self.append_audit(&audit_format, "current", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_provider_models_refresh(
        &mut self,
        provider: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        self.refresh_provider_health_statuses();
        self.refresh_user_model_access();
        self.refresh_user_routes();
        let Some(&inode) = self.provider_models_refresh.get(provider) else {
            return Err(libc::EINVAL.into());
        };
        self.update_dynamic_file(inode, "1\n");
        self.update_dynamic_file(
            self.last_control_inode,
            format!("provider/{provider}/model/refresh\n"),
        );
        let audit_format = format!("provider.{provider}.model");
        self.append_audit(&audit_format, "refresh", "refreshed");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_provider_enabled_current(
        &mut self,
        provider: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        if offset != 0 {
            return Err(libc::EINVAL.into());
        }
        let enabled = std::str::from_utf8(data).map_err(|_error| libc::EINVAL)?;
        let trimmed = enabled.trim();
        let (value, source) = match trimmed {
            "" => ("1\n", "default\n"),
            "0" => ("0\n", "current\n"),
            "1" => ("1\n", "current\n"),
            _ => return Err(libc::EINVAL.into()),
        };
        let Some(inodes) = self.provider_enabled.get(provider).copied() else {
            return Err(libc::EINVAL.into());
        };
        if let Some(inode) = inodes.current {
            self.update_dynamic_file(inode, value);
        }
        if let Some(inode) = inodes.effective {
            self.update_dynamic_file(inode, value);
        }
        if let Some(inode) = inodes.source {
            self.update_dynamic_file(inode, source);
        }
        if let Some(inode) = inodes.status {
            let status = if value == "1\n" {
                "ready\n"
            } else {
                "disabled\n"
            };
            self.update_dynamic_file(inode, status);
        }
        self.refresh_user_model_access();
        self.refresh_user_routes();
        let audit_format = format!("provider.{provider}.enabled");
        self.append_audit(&audit_format, "current", "configured");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_provider_health_check(
        &mut self,
        provider: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        let provider_enabled = self.provider_enabled(provider);
        let status = if provider_enabled {
            "queued\n"
        } else {
            "disabled\n"
        };
        let latency_ms = "\n";
        let last_error = if provider_enabled {
            "daemon pending\n"
        } else {
            "provider disabled\n"
        };
        if let Some(inode) = self.provider_health_status_inode(provider) {
            self.update_dynamic_file(inode, status);
        }
        if let Some(&inode) = self.provider_health_latency_ms.get(provider) {
            self.update_dynamic_file(inode, latency_ms);
        }
        if let Some(&inode) = self.provider_health_last_error.get(provider) {
            self.update_dynamic_file(inode, last_error);
        }
        let Some(&inode) = self.provider_health_check.get(provider) else {
            return Err(libc::EINVAL.into());
        };
        self.update_dynamic_file(inode, "1\n");
        self.update_dynamic_file(
            self.last_control_inode,
            format!("provider/{provider}/health/check\n"),
        );
        let audit_format = format!("provider.{provider}.health");
        self.append_audit(&audit_format, "check", "queued");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn write_provider_secret_rotate(
        &mut self,
        provider: &str,
        offset: u64,
        data: &[u8],
    ) -> fuse3::Result<u32> {
        validate_control_write(offset, data)?;
        let Some(&rotate_inode) = self.provider_secret_rotate.get(provider) else {
            return Err(libc::EINVAL.into());
        };
        self.update_dynamic_file(rotate_inode, "1\n");
        if let Some(&inode) = self.provider_secret_active.get(provider) {
            self.update_dynamic_file(inode, secret_rotating_id(provider));
        }
        if let Some(&inode) = self.provider_secret_last_rotated.get(provider) {
            self.update_dynamic_file(inode, "pending\n");
        }
        if let Some(&inode) = self.provider_secret_next_rotation.get(provider) {
            self.update_dynamic_file(inode, "\n");
        }
        self.update_dynamic_file(
            self.last_control_inode,
            format!("provider/{provider}/secrets/rotate\n"),
        );
        let audit_format = format!("provider.{provider}.secrets");
        self.append_audit(&audit_format, "rotate", "requested");
        u32::try_from(data.len()).map_err(|_error| fuse3::Errno::from(libc::EFBIG))
    }

    fn provider_for_models_refresh_inode(&self, inode: Inode) -> Option<&'static str> {
        self.provider_models_refresh
            .iter()
            .find_map(|(&provider, &refresh_inode)| (refresh_inode == inode).then_some(provider))
    }

    fn provider_for_url_current_inode(&self, inode: Inode) -> Option<&'static str> {
        self.provider_url
            .iter()
            .find_map(|(&provider, inodes)| (inodes.current == Some(inode)).then_some(provider))
    }

    fn provider_for_enabled_current_inode(&self, inode: Inode) -> Option<&'static str> {
        self.provider_enabled
            .iter()
            .find_map(|(&provider, inodes)| (inodes.current == Some(inode)).then_some(provider))
    }

    fn provider_for_health_check_inode(&self, inode: Inode) -> Option<&'static str> {
        self.provider_health_check
            .iter()
            .find_map(|(&provider, &check_inode)| (check_inode == inode).then_some(provider))
    }

    fn provider_for_secret_rotate_inode(&self, inode: Inode) -> Option<&'static str> {
        self.provider_secret_rotate
            .iter()
            .find_map(|(&provider, &rotate_inode)| (rotate_inode == inode).then_some(provider))
    }

    pub fn refresh_provider_health_statuses(&mut self) {
        for provider in configured_provider_ids() {
            let status = if self.provider_enabled(provider) {
                "unknown\n"
            } else {
                "disabled\n"
            };
            if let Some(inode) = self.provider_health_status_inode(provider) {
                self.update_dynamic_file(inode, status);
            }
        }
    }

    pub fn refresh_user_model_access(&mut self) {
        for provider in configured_provider_ids() {
            let (allowed, reason) = self.provider_model_access(provider);
            let Some(inodes) = self.user_model_access.get(provider).copied() else {
                continue;
            };
            self.update_dynamic_file(inodes.allowed, allowed);
            self.update_dynamic_file(inodes.reason, reason);
        }
        self.refresh_user_model_index();
    }

    fn refresh_user_model_index(&mut self) {
        let available = PROVIDER_SPECS
            .iter()
            .filter(|provider| self.provider_model_access(provider.id).0 == "1\n")
            .map(provider_model_id)
            .collect::<Vec<_>>();
        if let Some(inode) = self.user_models_count_inode {
            self.update_dynamic_file(inode, format!("{}\n", available.len()));
        }
        if let Some(inode) = self.user_models_list_inode {
            self.update_dynamic_file(inode, newline_list(available.iter()));
        }
    }

    pub fn refresh_user_routes(&mut self) {
        for format in API_FORMATS {
            let route = self.route_for_format(format);
            self.update_user_route(format, &route);
        }
    }

    fn route_for_format(&self, format: &str) -> ApiRoute {
        if !API_FORMATS.contains(&format) {
            return ApiRoute::unsupported_format();
        }
        let preferred_provider = self
            .current_user_default_provider()
            .and_then(provider_spec)
            .filter(|provider| provider_supports_format(provider, format));
        let routed_provider = preferred_provider.or_else(|| {
            PROVIDER_SPECS.iter().copied().find(|provider| {
                provider_supports_format(provider, format)
                    && self.provider_model_access(provider.id).0 == "1\n"
            })
        });
        let provider = routed_provider.or_else(|| {
            PROVIDER_SPECS
                .iter()
                .copied()
                .find(|provider| provider_supports_format(provider, format))
        });
        let Some(provider) = provider else {
            return ApiRoute::unsupported_format();
        };
        if !self.is_provider_allowed(provider.id) {
            ApiRoute::new(provider.id, "", "policy_denied")
        } else if self.provider_model_access(provider.id) == ("0\n", "provider_disabled\n") {
            ApiRoute::new(provider.id, "", "provider_disabled")
        } else {
            ApiRoute::new(provider.id, provider.default_model, "ready")
        }
    }

    fn update_user_route(&mut self, format: &str, route: &ApiRoute) {
        let Some(inodes) = self.user_routes.get(format).copied() else {
            return;
        };
        self.update_dynamic_file(inodes.provider, format!("{}\n", route.provider));
        self.update_dynamic_file(inodes.model, format!("{}\n", route.model));
        self.update_dynamic_file(inodes.reason, format!("{}\n", route.reason));
    }

    pub fn current_route(&self, format: &str) -> (&str, &str, &str) {
        let Some(inodes) = self.user_routes.get(format) else {
            return ("", "", "unsupported_format");
        };
        let provider = self
            .nodes
            .get(&inodes.provider)
            .and_then(Node::content)
            .map_or("", str::trim);
        let model = self
            .nodes
            .get(&inodes.model)
            .and_then(Node::content)
            .map_or("", str::trim);
        let reason = self
            .nodes
            .get(&inodes.reason)
            .and_then(Node::content)
            .map_or("unsupported_format", str::trim);
        (provider, model, reason)
    }

    fn provider_model_access(&self, provider: &str) -> (&'static str, &'static str) {
        if !self.is_provider_allowed(provider) {
            return ("0\n", "policy_denied\n");
        }
        if !self.provider_enabled(provider) {
            return ("0\n", "provider_disabled\n");
        }
        ("1\n", "ready\n")
    }

    fn provider_enabled(&self, provider: &str) -> bool {
        let inode = self.provider_enabled_effective_inode(provider);
        inode
            .and_then(|inode| self.nodes.get(&inode))
            .and_then(Node::content)
            .is_some_and(|content| content.trim() == "1")
    }

    fn provider_enabled_effective_inode(&self, provider: &str) -> Option<Inode> {
        self.provider_enabled
            .get(provider)
            .and_then(|inodes| inodes.effective)
    }

    fn provider_health_status_inode(&self, provider: &str) -> Option<Inode> {
        self.provider_health_status.get(provider).copied()
    }

    pub fn current_route_is_allowed(&self, format: &str) -> bool {
        let (provider, model, reason) = self.current_route(format);
        !provider.is_empty()
            && !model.is_empty()
            && reason == "ready"
            && self.provider_model_access(provider).0 == "1\n"
    }
}
