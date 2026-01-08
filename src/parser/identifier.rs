// TODO: Should this just be a string?
// ALSO TODO: Does this belong in the parser? It could be that
// a string representation belongs in the scanner, but an identifier
// still belongs here in the parser.

#[derive(PartialEq, Clone, Debug)]
pub struct Identifier(Box<String>);

impl Identifier {
    pub fn new(identifier: &str) -> Identifier {
        Identifier(Box::new(identifier.to_owned()))
    }

    pub fn get(&self) -> String {
        *self.0.clone()
    }
}
