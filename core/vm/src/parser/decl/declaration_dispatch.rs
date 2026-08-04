impl Parser {
    pub(super) fn parse_declaration(&mut self) -> Result<Decl, ParseError> {
        let is_pub = if self.check(TokenType::Pub) {
            self.advance();
            true
        } else {
            false
        };

        if self.check(TokenType::Component) {
            let mut decl = self.parse_data_decl(DataKind::Component, true)?;
            decl.is_pub = is_pub;
            return Ok(Decl::Component(decl));
        }
        if self.check(TokenType::Resource) {
            let mut decl = self.parse_resource_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Resource(decl));
        }
        // `transient resource Name { … }` — soft keyword: runtime state
        // excluded from world_digest()/save_world() (tapes, caches)
        if self.check_ident_text("transient") && self.peek_at(1).ty == TokenType::Resource {
            self.advance();
            let mut decl = self.parse_resource_decl()?;
            decl.is_pub = is_pub;
            decl.transient = true;
            return Ok(Decl::Resource(decl));
        }
        if self.check(TokenType::Struct) {
            let mut decl = self.parse_data_decl(DataKind::Struct, false)?;
            decl.is_pub = is_pub;
            return Ok(Decl::Struct(decl));
        }
        if self.check_ident_text("intent")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LBrace
        {
            let mut decl = self.parse_intent_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Intent(decl));
        }
        if self.check_ident_text("law")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LParen
        {
            let mut decl = self.parse_law_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Law(decl));
        }
        if self.check_ident_text("resolver")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::For
        {
            let mut decl = self.parse_resolver_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Resolver(decl));
        }
        if self.check_ident_text("constraint")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::For
        {
            let mut decl = self.parse_constraint_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Constraint(decl));
        }
        if self.check(TokenType::Entity) && self.peek_at(1).ty == TokenType::Ident {
            let mut decl = self.parse_entity_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Entity(decl));
        }
        if self.check(TokenType::State) {
            let mut decl = self.parse_state_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::State(decl));
        }
        if self.check(TokenType::System) {
            let mut decl = self.parse_system_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::System(decl));
        }
        if self.check(TokenType::On)
            || (self.check(TokenType::Async) && self.peek_at(1).ty == TokenType::On)
        {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on event handlers".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::OnHandler(self.parse_on_handler()?));
        }
        if self.check(TokenType::Pure) {
            let mut decl = self.parse_pure_fn()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.is_effect_annotated_fn() {
            let mut decl = self.parse_effect_fn()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.check(TokenType::Event) {
            let mut decl = self.parse_event_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Event(decl));
        }
        if self.check(TokenType::Fn)
            || (self.check(TokenType::Async) && self.peek_at(1).ty == TokenType::Fn)
        {
            let mut decl = self.parse_fn_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.check(TokenType::Type) {
            let mut decl = self.parse_type_decl_or_alias()?;
            match &mut decl {
                Decl::Type(t) => t.is_pub = is_pub,
                Decl::TypeAlias(a) => a.is_pub = is_pub,
                _ => {}
            }
            return Ok(decl);
        }
        if self.check(TokenType::Use) {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on `use` statements".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::Use(self.parse_use()?));
        }
        // Soft keyword: `migrate Component(old) { ... }` (schema migration,
        // list item #5). Only treated as a declaration when followed by an
        // identifier and `(` so `migrate` stays usable as a plain name.
        if self.check(TokenType::Ident)
            && self.peek().value.as_str() == Some("migrate")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LParen
        {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on migrations".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::Migration(self.parse_migration_decl()?));
        }
        // Soft keywords: `serial phase P [ ... ]` — a phase whose member
        // systems never share a parallel batch with each other (dogfood
        // feature seq 83). Only treated as a declaration when the full
        // `serial phase <Name>` shape is present, so `serial` stays usable
        // as a plain name.
        if self.check(TokenType::Ident)
            && self.peek().value.as_str() == Some("serial")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(1).value.as_str() == Some("phase")
            && self.peek_at(2).ty == TokenType::Ident
        {
            self.advance(); // consume `serial`
            let mut decl = self.parse_phase_decl()?;
            decl.serial = true;
            decl.is_pub = is_pub;
            return Ok(Decl::Phase(decl));
        }
        if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("phase") {
            let next = self.peek_at(1);
            if next.ty == TokenType::Ident {
                let mut decl = self.parse_phase_decl()?;
                decl.is_pub = is_pub;
                return Ok(Decl::Phase(decl));
            }
        }
        if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("test") {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on tests".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            let next = self.peek_at(1);
            if next.ty == TokenType::String || next.ty == TokenType::Ident {
                return Ok(Decl::Test(self.parse_test_decl()?));
            }
        }
        if is_pub {
            // `pub let NAME = ...` — a module constant exported like a pub fn.
            if self.check(TokenType::Let) {
                let stmt = self.parse_statement()?;
                if let Stmt::Let(mut l) = stmt {
                    if l.tuple_destructure || l.names.len() != 1 {
                        return Err(ParseError {
                            message: "`pub let` requires a single name (no tuple destructuring)"
                                .to_string(),
                            line: l.span.line,
                            col: l.span.col,
                        });
                    }
                    if l.mutable {
                        return Err(ParseError {
                            message: "`pub let` must be immutable — exported module state would hide shared mutation; use a resource for shared mutable state".to_string(),
                            line: l.span.line,
                            col: l.span.col,
                        });
                    }
                    l.is_pub = true;
                    return Ok(Decl::Stmt(Stmt::Let(l)));
                }
                return Err(ParseError {
                    message: "`pub` is only allowed on top-level declarations".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Err(ParseError {
                message: "`pub` is only allowed on top-level declarations".to_string(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        Ok(Decl::Stmt(self.parse_statement()?))
    }
}
