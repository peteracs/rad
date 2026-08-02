use super::*;

const MAX_RECOVERY_ERRORS: usize = 200;

impl Parser {
    pub fn parse(&mut self) -> Program {
        let mut declarations = Vec::new();
        let mut max_recovery_steps = 10000usize;

        while !self.check(TokenType::Eof) && max_recovery_steps > 0 {
            match self.parse_declaration() {
                Ok(decl) => declarations.push(decl),
                Err(e) => {
                    self.push_error(e);
                    if self.errors.len() >= MAX_RECOVERY_ERRORS {
                        self.push_error(ParseError {
                            message: format!(
                                "Stopping parse recovery after {} errors",
                                MAX_RECOVERY_ERRORS
                            ),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                        break;
                    }

                    // Synchronize to the next top-level declaration
                    self.synchronize(&[
                        TokenType::Component,
                        TokenType::Struct,
                        TokenType::Entity,
                        TokenType::State,
                        TokenType::System,
                        TokenType::Event,
                        TokenType::On,
                        TokenType::Fn,
                        TokenType::Pure,
                        TokenType::Type,
                        TokenType::Use,
                        TokenType::Pub,
                        TokenType::Let,
                    ]);

                    declarations.push(Decl::Error);
                    max_recovery_steps -= 1;
                }
            }
        }

        Program { declarations }
    }
}
