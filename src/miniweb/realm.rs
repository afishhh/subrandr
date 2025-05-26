use std::rc::Rc;

pub mod symbol;
use symbol::{Symbol, SymbolInterner};

#[derive(Debug)]
pub struct Realm {
    symbols: SymbolInterner,
}

impl Realm {
    pub fn create() -> Rc<Self> {
        Rc::new(Self {
            symbols: SymbolInterner::new(),
        })
    }

    pub fn symbol(&self, value: &str) -> Symbol {
        self.symbols.intern(value)
    }
}
