use super::*;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProviderRoute {
    pub(crate) transport: ResolvedTransport,
    pub(crate) key_slot: Option<String>,
}
pub(crate) fn provider_route(
    config: &RunnerProviderConfig,
    provider: &str,
    model: &str,
    route_text: Option<&str>,
) -> Result<ProviderRoute, String> {
    let Some(route_text) = route_text else {
        return Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: config.base_url.clone(),
            },
            key_slot: None,
        });
    };
    let table = parse_model_transport_route_table(route_text)?;
    let target = ProviderRouteTarget::from_provider_model(provider, model, &config.base_url)?;
    let group = table
        .rules
        .iter()
        .find(|rule| rule.matches(&target))
        .map(|rule| rule.group.as_str())
        .or(table.fallback.as_deref());
    let Some(group) = group else {
        return Ok(ProviderRoute {
            transport: ResolvedTransport::Direct {
                base_url: config.base_url.clone(),
            },
            key_slot: None,
        });
    };
    let action = table
        .groups
        .get(group)
        .cloned()
        .unwrap_or_else(|| RouteGroupAction::named_default(group));
    Ok(ProviderRoute {
        transport: action.transport.into_transport(&config.base_url, group)?,
        key_slot: action.key_slot,
    })
}
#[cfg(test)]
pub(crate) fn provider_transport(
    config: &RunnerProviderConfig,
    route_text: Option<&str>,
) -> Result<ResolvedTransport, String> {
    provider_route(config, "", "", route_text).map(|route| route.transport)
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModelTransportRouteTable {
    pub(crate) groups: BTreeMap<String, RouteGroupAction>,
    pub(crate) rules: Vec<RouteRule>,
    pub(crate) fallback: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RouteGroupAction {
    pub(crate) transport: RouteAction,
    pub(crate) key_slot: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RouteAction {
    Direct,
    Http {
        base_url: String,
    },
    Unix {
        socket_path: String,
        base_url: String,
    },
}
impl RouteAction {
    fn named_default(group: &str) -> Self {
        if group == "direct" || group == "must_direct" {
            Self::Direct
        } else {
            Self::Http {
                base_url: group.to_owned(),
            }
        }
    }
    fn into_transport(
        self,
        provider_base_url: &str,
        group: &str,
    ) -> Result<ResolvedTransport, String> {
        match self {
            Self::Direct => Ok(ResolvedTransport::Direct {
                base_url: provider_base_url.to_owned(),
            }),
            Self::Http { base_url } if is_url(&base_url) => {
                Ok(ResolvedTransport::Http { base_url })
            }
            Self::Http { .. } => Err(format!("route group {group} is not defined")),
            Self::Unix {
                socket_path,
                base_url,
            } => Ok(ResolvedTransport::Unix {
                base_url,
                socket_path,
            }),
        }
    }
}
impl RouteGroupAction {
    fn named_default(group: &str) -> Self {
        Self {
            transport: RouteAction::named_default(group),
            key_slot: None,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RouteRule {
    pub(crate) matcher: RouteMatcher,
    pub(crate) group: String,
}
impl RouteRule {
    fn matches(&self, target: &ProviderRouteTarget) -> bool {
        match self.matcher {
            RouteMatcher::Domain(ref patterns) => patterns
                .iter()
                .any(|pattern| domain_matches(pattern, &target.host)),
            RouteMatcher::DestinationIp(ref patterns) => target
                .ip
                .as_ref()
                .is_some_and(|ip| patterns.iter().any(|pattern| ip_matches(pattern, ip))),
            RouteMatcher::ProcessName(ref names) => env::args()
                .next()
                .and_then(|path| PathBuf::from(path).file_name().map(ToOwned::to_owned))
                .and_then(|value| value.to_str().map(str::to_owned))
                .is_some_and(|name| names.iter().any(|pattern| pattern == &name)),
            RouteMatcher::Provider(ref patterns) => patterns
                .iter()
                .any(|pattern| route_pattern_matches(pattern, &target.provider)),
            RouteMatcher::Model(ref patterns) => patterns
                .iter()
                .any(|pattern| route_pattern_matches(pattern, &target.model)),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RouteMatcher {
    Domain(Vec<String>),
    DestinationIp(Vec<String>),
    ProcessName(Vec<String>),
    Provider(Vec<String>),
    Model(Vec<String>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderRouteTarget {
    provider: String,
    model: String,
    host: String,
    ip: Option<IpAddr>,
}
impl ProviderRouteTarget {
    fn from_provider_model(provider: &str, model: &str, base_url: &str) -> Result<Self, String> {
        let host = cortexfs::provider_host_from_base_url(base_url)
            .ok_or_else(|| "invalid provider base_url".to_owned())?;
        let ip = host.parse::<IpAddr>().ok();
        Ok(Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            host,
            ip,
        })
    }
}
pub(crate) fn parse_model_transport_route_table(
    content: &str,
) -> Result<ModelTransportRouteTable, String> {
    let mut table = ModelTransportRouteTable {
        groups: BTreeMap::from([
            (
                "direct".to_owned(),
                RouteGroupAction::named_default("direct"),
            ),
            (
                "must_direct".to_owned(),
                RouteGroupAction::named_default("must_direct"),
            ),
        ]),
        rules: Vec::new(),
        fallback: None,
    };
    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(value, _comment)| value)
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("fallback:") {
            table.fallback = Some(valid_route_name(value.trim(), line_number)?);
            continue;
        }
        let Some((left, right)) = line.split_once("->") else {
            return Err(format!("invalid route line {line_number}: missing ->"));
        };
        let left = left.trim();
        let right = right.trim();
        if let Some(name) = call_arg(left, "group") {
            table.groups.insert(
                valid_route_name(name.trim(), line_number)?,
                parse_route_action(right, line_number)?,
            );
            continue;
        }
        table.rules.push(RouteRule {
            matcher: parse_route_matcher(left, line_number)?,
            group: valid_route_name(right, line_number)?,
        });
    }
    Ok(table)
}
pub(crate) fn parse_route_matcher(value: &str, line: usize) -> Result<RouteMatcher, String> {
    if let Some(args) = call_arg(value, "domain") {
        return Ok(RouteMatcher::Domain(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "dip") {
        return Ok(RouteMatcher::DestinationIp(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "pname") {
        return Ok(RouteMatcher::ProcessName(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "provider") {
        return Ok(RouteMatcher::Provider(parse_route_list(args, line)?));
    }
    if let Some(args) = call_arg(value, "model") {
        return Ok(RouteMatcher::Model(parse_route_list(args, line)?));
    }
    Err(format!("invalid route matcher on line {line}"))
}
pub(crate) fn parse_route_action(value: &str, line: usize) -> Result<RouteGroupAction, String> {
    let mut transport = None;
    let mut key_slot = None;
    for part in split_route_action_parts(value) {
        if part == "direct" || part == "must_direct" {
            transport = Some(RouteAction::Direct);
            continue;
        }
        if let Some(url) = call_arg(part, "http") {
            let url = url.trim();
            if is_url(url) {
                transport = Some(RouteAction::Http {
                    base_url: url.to_owned(),
                });
                continue;
            }
            return Err(format!("invalid http group on line {line}"));
        }
        if let Some(args) = call_arg(part, "unix") {
            let values = parse_route_list(args, line)?;
            let Some(socket_path) = values.first() else {
                return Err(format!("invalid unix group on line {line}"));
            };
            if !is_safe_absolute_unix_socket_path(socket_path) {
                return Err(format!("invalid unix socket path on line {line}"));
            }
            let base_url = values
                .get(1)
                .cloned()
                .unwrap_or_else(|| "http://localhost/v1".to_owned());
            if !is_url(&base_url) {
                return Err(format!("invalid unix base_url on line {line}"));
            }
            transport = Some(RouteAction::Unix {
                socket_path: socket_path.to_owned(),
                base_url,
            });
            continue;
        }
        if let Some(slot) = call_arg(part, "key") {
            key_slot = Some(valid_route_name(slot.trim(), line)?);
            continue;
        }
        return Err(format!("invalid group action on line {line}"));
    }
    let Some(transport) = transport else {
        return Err(format!("route group missing transport on line {line}"));
    };
    Ok(RouteGroupAction {
        transport,
        key_slot,
    })
}
pub(crate) fn split_route_action_parts(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                let part = value.get(start..index).unwrap_or_default().trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = value.get(start..).unwrap_or_default().trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}
pub(crate) fn parse_route_list(value: &str, line: usize) -> Result<Vec<String>, String> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        Err(format!("empty route list on line {line}"))
    } else {
        Ok(values)
    }
}
pub(crate) fn call_arg<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')
}
pub(crate) fn valid_route_name(value: &str, line: usize) -> Result<String, String> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Ok(value.to_owned())
    } else {
        Err(format!("invalid route group on line {line}"))
    }
}
pub(crate) fn domain_matches(pattern: &str, host: &str) -> bool {
    if let Some(geosite) = pattern.strip_prefix("geosite:") {
        return geosite == "cn"
            && host
                .rsplit('.')
                .next()
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case("cn"));
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}
pub(crate) fn route_pattern_matches(pattern: &str, value: &str) -> bool {
    pattern == "*"
        || pattern == value
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| value.starts_with(prefix))
}
pub(crate) fn ip_matches(pattern: &str, ip: &IpAddr) -> bool {
    match pattern {
        "geoip:private" => match *ip {
            IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
        },
        "geoip:cn" => false,
        value => value.parse::<IpAddr>().is_ok_and(|target| &target == ip),
    }
}
pub(crate) fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}
pub(crate) fn is_safe_absolute_unix_socket_path(value: &str) -> bool {
    value.starts_with('/') && !value.bytes().any(|byte| byte.is_ascii_control())
}
