/// A command is a message that starts with a slash and has a name.
///
/// Examples: `/start`, `/help@MyBot`, `/echo hello world`.
#[derive(Debug)]
pub struct Command {
    /// The name of the command.
    ///
    /// `/start` -> `start`
    pub name: String,
    /// The bot that the command was sent to.
    ///
    /// `/help@MyBot` -> `MyBot`
    pub via: Option<String>,
    /// The argument of the command.
    ///
    /// `/echo hello world` -> `hello world`
    pub arg: Option<String>,
}

/// Parse a command from the given text.
pub fn parse_command(text: &str) -> Option<Command> {
    // Commands must start with a slash
    if !text.starts_with('/') {
        return None;
    }

    // Split the command name and argument
    // (e.g. `/help hello world` -> `/help` and `hello world`)
    // NOTE: text[1..] is used to skip the slash
    let mut iter = text[1..].splitn(2, ' ');
    let name = iter.next()?;
    let arg = iter.next();

    // Split the command name and bot name
    // (e.g. `/help@MyBot` -> `/help` and `MyBot`)
    let mut iter = name.splitn(2, '@');

    Some(Command {
        name: iter.next()?.to_string(),
        via: iter.next().map(|s| s.to_string()),
        arg: arg.map(|s| s.to_string()),
    })
}
