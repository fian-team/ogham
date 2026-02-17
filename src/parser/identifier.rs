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
