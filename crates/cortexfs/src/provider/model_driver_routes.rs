/// Model driver call site used to select a driver route.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModelDriverUseCase {
    /// Fallback route when no use-case-specific route is available.
    Default,
    /// One-shot execution through `model/<provider>/<model>`.
    Exec,
    /// Stateful model socket traffic through `model/<provider>/<model>.sock`.
    Socket,
    /// Agent-owned model calls.
    Agent,
}

/// Error while parsing `model/<provider>/<model>.d/driver`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelDriverRouteError {
    /// The route table has no usable driver declarations.
    Empty,
    /// A route-table line is missing `=`.
    MissingEquals { line: usize },
    /// A route-table key is not one of default, exec, socket, or agent.
    UnknownUseCase { line: usize, value: String },
    /// A route-table key appears more than once.
    DuplicateUseCase { line: usize, value: String },
    /// A driver list is empty or has an empty comma element.
    EmptyDriver { line: usize },
    /// A driver name is not a valid stable component.
    InvalidDriverName { line: usize, value: String },
}

/// Parsed `driver` control-file route table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelDriverRoutingTable {
    routes: HashMap<ModelDriverUseCase, Vec<String>>,
}

impl ModelDriverUseCase {
    /// Parses one route-table key.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "exec" => Some(Self::Exec),
            "socket" => Some(Self::Socket),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    /// Returns the route-table key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Exec => "exec",
            Self::Socket => "socket",
            Self::Agent => "agent",
        }
    }
}

impl ModelDriverRoutingTable {
    /// Creates an empty driver routing table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one ordered route list.
    pub fn insert(&mut self, use_case: ModelDriverUseCase, drivers: Vec<String>) {
        self.routes.insert(use_case, drivers);
    }

    /// Returns the exact route list for one use case.
    #[must_use]
    pub fn get(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.routes.get(&use_case).map(Vec::as_slice)
    }

    /// Returns the route list for a use case, falling back to `default`.
    #[must_use]
    pub fn drivers_for(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.get(use_case)
            .or_else(|| self.get(ModelDriverUseCase::Default))
    }

    /// Returns the first selected driver for a use case.
    #[must_use]
    pub fn primary_driver_for(&self, use_case: ModelDriverUseCase) -> Option<&str> {
        self.drivers_for(use_case)
            .and_then(|drivers| drivers.first())
            .map(String::as_str)
    }

    /// Returns whether no route is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub(crate) fn route_value(&self, use_case: ModelDriverUseCase) -> String {
        self.get(use_case)
            .map(|drivers| drivers.join(","))
            .unwrap_or_default()
    }
}

/// Parses `model/<provider>/<model>.d/driver`.
///
/// A legacy single-line value such as `debug` is treated as `default=debug`.
/// Route-table form supports `default`, `exec`, `socket`, and `agent` keys with
/// comma-separated drivers in priority order.
pub fn parse_model_driver_routes(
    content: &str,
) -> Result<ModelDriverRoutingTable, ModelDriverRouteError> {
    let significant = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let value = line.trim();
            (!value.is_empty() && !value.starts_with('#')).then_some((index + 1, value))
        })
        .collect::<Vec<_>>();

    if significant.is_empty() {
        return Err(ModelDriverRouteError::Empty);
    }

    if significant.len() == 1 {
        let Some((line, driver)) = significant.first().copied() else {
            return Err(ModelDriverRouteError::Empty);
        };
        if !driver.contains('=') {
            return parse_driver_list(line, driver).map(|drivers| {
                let mut table = ModelDriverRoutingTable::new();
                table.insert(ModelDriverUseCase::Default, drivers);
                table
            });
        }
    }

    let mut table = ModelDriverRoutingTable::new();
    for (line, route) in significant {
        let Some((raw_key, raw_drivers)) = route.split_once('=') else {
            return Err(ModelDriverRouteError::MissingEquals { line });
        };
        let key = raw_key.trim();
        let Some(use_case) = ModelDriverUseCase::parse(key) else {
            return Err(ModelDriverRouteError::UnknownUseCase {
                line,
                value: key.to_owned(),
            });
        };
        if table.get(use_case).is_some() {
            return Err(ModelDriverRouteError::DuplicateUseCase {
                line,
                value: key.to_owned(),
            });
        }
        table.insert(use_case, parse_driver_list(line, raw_drivers)?);
    }

    if table.is_empty() {
        Err(ModelDriverRouteError::Empty)
    } else {
        Ok(table)
    }
}

fn parse_driver_list(line: usize, value: &str) -> Result<Vec<String>, ModelDriverRouteError> {
    let mut drivers = Vec::new();
    for raw_driver in value.split(',') {
        let driver = raw_driver.trim();
        if driver.is_empty() {
            return Err(ModelDriverRouteError::EmptyDriver { line });
        }
        if !is_object_name(driver) {
            return Err(ModelDriverRouteError::InvalidDriverName {
                line,
                value: driver.to_owned(),
            });
        }
        drivers.push(driver.to_owned());
    }
    if drivers.is_empty() {
        Err(ModelDriverRouteError::EmptyDriver { line })
    } else {
        Ok(drivers)
    }
}
