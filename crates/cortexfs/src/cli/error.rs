#[macro_export]
/// Declares a small CLI error type with stable exit-style payloads.
macro_rules! define_simple_cli_error {
    ($name:ident) => {
        #[derive(Debug, Eq, PartialEq)]
        /// Generated CLI error struct for `$name`.
        struct $name {
            code: u8,
            message: String,
        }

        impl $name {
            fn usage(message: impl Into<String>) -> Self {
                Self {
                    code: 2,
                    message: message.into(),
                }
            }

            fn unavailable(message: impl Into<String>) -> Self {
                Self {
                    code: 69,
                    message: message.into(),
                }
            }
        }
    };
}
