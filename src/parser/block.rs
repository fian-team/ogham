use super::statement::*;

#[derive(PartialEq, Clone, Debug)]
pub struct Block {
  pub statement_list: Vec<Statement>,
}

impl Block {
  pub fn new() -> Block {
    Block {
      statement_list: Vec::new(),
    }
  }
}
