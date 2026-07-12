pub mod bootstrap;
pub(crate) mod executor;
pub mod install;
pub mod layout;
pub mod metadata;
pub mod receipt;
pub mod residue;
pub(crate) mod runner;
pub mod uninstall;

/// Deserializes a present optional wire field while rejecting explicit null.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    struct NonNull<T>(std::marker::PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for NonNull<T>
    where
        T: serde::Deserialize<'de>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a non-null value")
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            T::deserialize(deserializer)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Err(E::custom("null is not allowed"))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Err(E::custom("null is not allowed"))
        }
    }

    deserializer
        .deserialize_option(NonNull(std::marker::PhantomData))
        .map(Some)
}

/// Runs the installed object executor.
#[doc(hidden)]
#[must_use]
pub fn runner_main() -> std::process::ExitCode {
    executor::main()
}
