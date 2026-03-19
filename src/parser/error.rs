use std::fmt;

/// Multiple parse errors collected across a whole file.
/// Using a `Vec` instead of the first-error-wins `String` means the parser
/// can keep going after a missing semicolon and report everything at once.
pub struct ParseErrors(pub Vec<String>);

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", e)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
