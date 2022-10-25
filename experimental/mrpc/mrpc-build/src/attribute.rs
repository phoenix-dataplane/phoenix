/// Attributes that will be added to `mod` and `struct` items.
#[derive(Debug, Default, Clone)]
pub struct Attributes {
    /// `mod` attributes.
    module: Vec<(String, String)>,
    /// `struct` attributes.
    structure: Vec<(String, String)>,
}

impl Attributes {
    pub(crate) fn for_mod(&self, name: &str) -> Vec<syn::Attribute> {
        generate_attributes(name, &self.module)
    }

    pub(crate) fn for_struct(&self, name: &str) -> Vec<syn::Attribute> {
        generate_attributes(name, &self.structure)
    }

    /// Add an attribute that will be added to `mod` items matching the given pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// # use mrpc_build::attribute::*;
    /// let mut attributes = Attributes::default();
    /// attributes.push_mod("my.proto.package", r#"#[cfg(feature = "server")]"#);
    /// ```
    pub fn push_mod(&mut self, pattern: impl Into<String>, attr: impl Into<String>) {
        self.module.push((pattern.into(), attr.into()));
    }

    /// Add an attribute that will be added to `struct` items matching the given pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// # use mrpc_build::attribute::*;
    /// let mut attributes = Attributes::default();
    /// attributes.push_struct("EchoService", "#[derive(PartialEq)]");
    /// ```
    pub fn push_struct(&mut self, pattern: impl Into<String>, attr: impl Into<String>) {
        self.structure.push((pattern.into(), attr.into()));
    }
}

// Generates attributes given a list of (`pattern`, `attribute`) pairs. If `pattern` matches `name`, `attribute` will be included.
fn generate_attributes<'a>(
    name: &str,
    attrs: impl IntoIterator<Item = &'a (String, String)>,
) -> Vec<syn::Attribute> {
    attrs
        .into_iter()
        .filter(|(matcher, _)| match_name(matcher, name))
        .flat_map(|(_, attr)| {
            // attributes cannot be parsed directly, so we pretend they're on a struct
            syn::parse_str::<syn::DeriveInput>(&format!("{}\nstruct fake;", attr))
                .unwrap()
                .attrs
        })
        .collect::<Vec<_>>()
}

// Checks whether a path pattern matches a given path.
pub(crate) fn match_name(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() {
        false
    } else if pattern == "." || pattern == path {
        true
    } else {
        let pattern_segments = pattern.split('.').collect::<Vec<_>>();
        let path_segments = path.split('.').collect::<Vec<_>>();

        if &pattern[..1] == "." {
            // prefix match
            if pattern_segments.len() > path_segments.len() {
                false
            } else {
                pattern_segments[..] == path_segments[..pattern_segments.len()]
            }
        // suffix match
        } else if pattern_segments.len() > path_segments.len() {
            false
        } else {
            pattern_segments[..] == path_segments[path_segments.len() - pattern_segments.len()..]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_name() {
        assert!(match_name(".", ".my.protos"));
        assert!(match_name(".", ".protos"));

        assert!(match_name(".my", ".my"));
        assert!(match_name(".my", ".my.protos"));
        assert!(match_name(".my.protos.Service", ".my.protos.Service"));

        assert!(match_name("Service", ".my.protos.Service"));

        assert!(!match_name(".m", ".my.protos"));
        assert!(!match_name(".p", ".protos"));

        assert!(!match_name(".my", ".myy"));
        assert!(!match_name(".protos", ".my.protos"));
        assert!(!match_name(".Service", ".my.protos.Service"));

        assert!(!match_name("service", ".my.protos.Service"));
    }
}
