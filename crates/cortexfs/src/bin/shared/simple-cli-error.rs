macro_rules! define_simple_cli_error {
    ($name:ident) => {
        #[derive(Debug, Eq, PartialEq)]
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
