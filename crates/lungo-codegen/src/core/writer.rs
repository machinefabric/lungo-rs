//! An indenting source writer that produces stable formatting, for every generated language.

/// An indenting source writer.
#[derive(Default)]
pub struct Writer {
    out: String,
    indent: usize,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        let s = s.as_ref();
        if s.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    pub fn open(&mut self, s: impl AsRef<str>) {
        self.line(s);
        self.indent += 1;
    }

    pub fn close(&mut self, s: impl AsRef<str>) {
        self.indent -= 1;
        self.line(s);
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    pub fn finish(self) -> String {
        self.out
    }
}
