impl Parser {
    pub(super) fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_or()?;
        while self.check(TokenType::PipeOp) {
            self.advance();
            let right = self.parse_or()?;
            let span = left.span().clone();
            left = Expr::Pipe(Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.check(TokenType::Or) {
            self.advance();
            let right = self.parse_and()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), BinOp::Or, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_equality()?;
        while self.check(TokenType::And) {
            self.advance();
            let right = self.parse_equality()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), BinOp::And, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while self.check_any(&[TokenType::Eq, TokenType::Neq]) || self.check_ident_text("is") {
            let op = match self.peek().ty {
                TokenType::Eq => BinOp::Eq,
                TokenType::Neq => BinOp::Ne,
                _ => BinOp::Is,
            };
            self.advance();
            let right = self.parse_comparison()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitor()?;
        while self.check_any(&[TokenType::Lt, TokenType::Gt, TokenType::Lte, TokenType::Gte]) {
            let op = match self.peek().ty {
                TokenType::Lt => BinOp::Lt,
                TokenType::Gt => BinOp::Gt,
                TokenType::Lte => BinOp::Le,
                TokenType::Gte => BinOp::Ge,
                _ => unreachable!(),
            };
            self.advance();
            let right = self.parse_bitor()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    // Bitwise operators bind tighter than comparisons (Rust-style), so
    // `mask & bit == 0` means `(mask & bit) == 0`, and looser than
    // arithmetic, so `a & b + c` means `a & (b + c)`.
    fn parse_bitor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitxor()?;
        while self.check(TokenType::Pipe) {
            self.advance();
            let right = self.parse_bitxor()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), BinOp::BitOr, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitand()?;
        while self.check(TokenType::Caret) {
            self.advance();
            let right = self.parse_bitand()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), BinOp::BitXor, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_shift()?;
        while self.check(TokenType::Amp) {
            self.advance();
            let right = self.parse_shift()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), BinOp::BitAnd, Box::new(right), span);
        }
        Ok(left)
    }

    /// `>>` is lexed as two `Gt` tokens (a single GtGt token would break
    /// nested generics like `map<str, list<int>>`), so we only treat it as
    /// a right shift when the two tokens are physically adjacent.
    fn at_shr(&self) -> bool {
        let a = self.peek();
        let b = self.peek_at(1);
        a.ty == TokenType::Gt && b.ty == TokenType::Gt && a.span.1 == b.span.0
    }

    // Shifts bind tighter than `&` and looser than `+` (C/Rust-style), so
    // `base + off << 3` means `(base + off) << 3` and `x << 2 & m` means
    // `(x << 2) & m`. Note: at statement level `xs << v` is list append —
    // the statement parser rewrites a top-level Shl into a push.
    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_addition()?;
        loop {
            let op = if self.check(TokenType::LessLess) {
                self.advance();
                BinOp::Shl
            } else if self.at_shr() {
                self.advance();
                self.advance();
                BinOp::Shr
            } else {
                break;
            };
            let right = self.parse_addition()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplication()?;
        while self.check_any(&[TokenType::Plus, TokenType::Minus]) {
            let op = if self.peek().ty == TokenType::Plus {
                BinOp::Add
            } else {
                BinOp::Sub
            };
            self.advance();
            let right = self.parse_multiplication()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        while self.check_any(&[TokenType::Star, TokenType::Slash, TokenType::Percent]) {
            let op = match self.peek().ty {
                TokenType::Star => BinOp::Mul,
                TokenType::Slash => BinOp::Div,
                TokenType::Percent => BinOp::Mod,
                _ => unreachable!(),
            };
            self.advance();
            let right = self.parse_unary()?;
            let span = left.span().clone();
            left = Expr::Binary(Box::new(left), op, Box::new(right), span);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.check(TokenType::Await) {
            let span = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::Await(Box::new(operand), span));
        }
        if self.check(TokenType::Async) {
            let span = self.span();
            self.advance();
            let callee = self.parse_postfix()?;
            match callee {
                Expr::Call(target, args, _) => {
                    return Ok(Expr::AsyncCall(target, args, span));
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected function call after `async`".to_string(),
                        line: span.line,
                        col: span.col,
                    });
                }
            }
        }
        if self.check(TokenType::Minus) {
            let span = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(UnaryOp::Neg, Box::new(operand), span));
        }
        if self.check_any(&[TokenType::Not, TokenType::Bang]) {
            let span = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(UnaryOp::Not, Box::new(operand), span));
        }
        if self.check(TokenType::Tilde) {
            let span = self.span();
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary(UnaryOp::BitNot, Box::new(operand), span));
        }
        self.parse_postfix()
    }
}
