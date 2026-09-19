//! L3 type catalog — first-binary creatable models (003 MVP).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L3Model {
    KvStore,
    RelationalTable,
    DocumentStore,
}

impl L3Model {
    pub fn name(self) -> &'static str {
        match self {
            Self::KvStore => "K/V Store",
            Self::RelationalTable => "Relational Table",
            Self::DocumentStore => "Document Store",
        }
    }

    pub fn first_binary_creatable() -> &'static [L3Model] {
        &[Self::KvStore, Self::RelationalTable, Self::DocumentStore]
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pub name: String,
    pub model: L3Model,
    pub multi_active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub containers: Vec<Container>,
}

impl Catalog {
    pub fn create(&mut self, name: impl Into<String>, model: L3Model, multi_active: bool) -> Result<(), &'static str> {
        if multi_active {
            return Err("multi_active_unsupported");
        }
        self.containers.push(Container {
            name: name.into(),
            model,
            multi_active: false,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_l3_creatable() {
        assert_eq!(L3Model::first_binary_creatable().len(), 3);
        let mut c = Catalog::default();
        c.create("kv", L3Model::KvStore, false).unwrap();
        c.create("t", L3Model::RelationalTable, false).unwrap();
        c.create("docs", L3Model::DocumentStore, false).unwrap();
        assert_eq!(c.create("x", L3Model::KvStore, true), Err("multi_active_unsupported"));
    }
}
