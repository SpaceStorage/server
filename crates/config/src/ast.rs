#[derive(Debug, Clone)]
pub enum Arg {
    Ident(String),
    Number(u64),
    Size(u64),
    DurationMs(u64),
    String(String),
    Path(String),
}

impl Arg {
    pub fn as_str(&self) -> String {
        match self {
            Self::Ident(s) | Self::String(s) | Self::Path(s) => s.clone(),
            Self::Number(n) => n.to_string(),
            Self::Size(n) => n.to_string(),
            Self::DurationMs(n) => n.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    pub args: Vec<Arg>,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub name: String,
    pub args: Vec<Arg>,
    pub items: Vec<Item>,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub enum Item {
    Directive(Directive),
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct Document {
    pub items: Vec<Item>,
}
