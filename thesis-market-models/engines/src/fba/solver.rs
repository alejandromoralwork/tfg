use std::fmt::Write;

/// Minimal LP builder that emits a textual LP model in simple LP format.
/// This is a lightweight encoder used to write model files for external solvers.
#[derive(Default, Debug)]
pub struct LPBuilder {
    objective: String,
    constraints: Vec<String>,
    bounds: Vec<String>,
    binaries: Vec<String>,
}

impl LPBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_objective(&mut self, expr: &str) {
        self.objective = expr.to_string();
    }

    pub fn add_constraint(&mut self, name: &str, expr: &str) {
        self.constraints.push(format!("{}: {}", name, expr));
    }

    pub fn add_bound(&mut self, bound: &str) {
        self.bounds.push(bound.to_string());
    }

    pub fn add_binary(&mut self, name: &str) {
        self.binaries.push(name.to_string());
    }

    /// Emit LP model as string in a simple format.
    pub fn to_lp(&self) -> String {
        let mut out = String::new();
        writeln!(&mut out, "Minimize").ok();
        writeln!(&mut out, "  obj: {}", self.objective).ok();
        writeln!(&mut out, "Subject To").ok();
        for c in &self.constraints {
            writeln!(&mut out, "  {}", c).ok();
        }
        if !self.bounds.is_empty() {
            writeln!(&mut out, "Bounds").ok();
            for b in &self.bounds {
                writeln!(&mut out, "  {}", b).ok();
            }
        }
        if !self.binaries.is_empty() {
            writeln!(&mut out, "Binaries").ok();
            for b in &self.binaries {
                writeln!(&mut out, "  {}", b).ok();
            }
        }
        writeln!(&mut out, "End").ok();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_builder_emits_basic_model() {
        let mut lp = LPBuilder::new();
        lp.set_objective("x1 + 2 x2");
        lp.add_constraint("c1", "x1 + x2 >= 10");
        lp.add_bound("x1 >= 0");
        lp.add_bound("x2 >= 0");
        lp.add_binary("z1");

        let s = lp.to_lp();
        assert!(s.contains("Minimize"));
        assert!(s.contains("Subject To"));
        assert!(s.contains("Bounds"));
        assert!(s.contains("Binaries"));
    }
}
