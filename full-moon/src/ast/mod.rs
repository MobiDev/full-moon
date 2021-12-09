#[macro_use]
mod parser_util;
mod parsers;
pub mod punctuated;
pub mod span;
mod update_positions;
mod visitors;

use crate::{
    plugins::{DefaultPlugin, Plugin, PluginInfo, PluginMod},
    tokenizer::{Symbol, Token, TokenReference, TokenType},
    util::*,
};
use derive_more::Display;
use full_moon_derive::{Node, Visit};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt};

use parser_util::{
    InternalAstError, OneOrMore, Parser, ParserState, ZeroOrMore, ZeroOrMoreDelimited,
};

use punctuated::{Pair, Punctuated};
use span::ContainedSpan;

#[cfg(feature = "roblox")]
pub mod types;
#[cfg(feature = "roblox")]
use types::*;

#[cfg(feature = "roblox")]
mod type_visitors;

#[cfg(feature = "lua52")]
pub mod lua52;
#[cfg(feature = "lua52")]
use lua52::*;

macro_rules! display_plugin_info {
    ($type:ident<$plugin:ident>, $value:expr) => {
        paste::item! {
            <<$plugin as Plugin>::[<$type Mod>] as PluginMod<$type<$plugin>>>::display($value)
        }
    };
}

/// A block of statements, such as in if/do/etc block
#[derive(Clone, Debug, Default, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(
    fmt = "{}{}",
    "display_optional_punctuated_vec(stmts)",
    "display_option(&last_stmt.as_ref().map(display_optional_punctuated))"
)]
pub struct Block<P: Plugin = DefaultPlugin> {
    stmts: Vec<(Stmt<P>, Option<TokenReference>)>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    last_stmt: Option<(LastStmt<P>, Option<TokenReference>)>,

    plugin_info: <<P as Plugin>::BlockMod as PluginMod<Block<P>>>::NodeInfo,
}

impl<P: Plugin> Block<P> {
    /// Creates an empty block
    pub fn new() -> Self {
        Self {
            stmts: Vec::new(),
            last_stmt: None,
            plugin_info: Default::default(),
        }
    }

    /// An iterator over the statements in the block, such as `local foo = 1`
    pub fn stmts(&self) -> impl Iterator<Item = &Stmt<P>> {
        self.stmts.iter().map(|(stmt, _)| stmt)
    }

    /// An iterator over the statements in the block, including any optional
    /// semicolon token reference present
    pub fn stmts_with_semicolon(&self) -> impl Iterator<Item = &(Stmt<P>, Option<TokenReference>)> {
        self.stmts.iter()
    }

    /// The last statement of the block if one exists, such as `return foo`
    pub fn last_stmt(&self) -> Option<&LastStmt<P>> {
        Some(&self.last_stmt.as_ref()?.0)
    }

    /// The last statement of the block if on exists, including any optional semicolon token reference present
    pub fn last_stmt_with_semicolon(&self) -> Option<&(LastStmt<P>, Option<TokenReference>)> {
        self.last_stmt.as_ref()
    }

    /// Returns a new block with the given statements
    /// Takes a vector of statements, followed by an optional semicolon token reference
    pub fn with_stmts(self, stmts: Vec<(Stmt<P>, Option<TokenReference>)>) -> Self {
        Self { stmts, ..self }
    }

    /// Returns a new block with the given last statement, if one is given
    /// Takes an optional last statement, with an optional semicolon
    pub fn with_last_stmt(self, last_stmt: Option<(LastStmt<P>, Option<TokenReference>)>) -> Self {
        Self { last_stmt, ..self }
    }
}

/// The last statement of a [`Block`]
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum LastStmt<P: Plugin = DefaultPlugin> {
    /// A `break` statement
    Break(TokenReference),
    /// A continue statement
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    Continue(TokenReference),
    /// A `return` statement
    Return(Return<P>),

    #[display(fmt = "{}", "display_plugin_info!(LastStmt<P>, self)")]
    Plugin(<<P as Plugin>::LastStmtMod as PluginMod<LastStmt<P>>>::NodeInfo),
}

/// A `return` statement
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}", token, returns)]
pub struct Return<P: Plugin = DefaultPlugin> {
    token: TokenReference,
    returns: Punctuated<Expression<P>>,

    plugin_info: <<P as Plugin>::ReturnMod as PluginMod<Return<P>>>::NodeInfo,
}

impl<P: Plugin> Return<P> {
    /// Creates a new empty Return
    /// Default return token is followed by a single space
    pub fn new() -> Self {
        Self {
            token: TokenReference::symbol("return ").unwrap(),
            returns: Punctuated::new(),

            plugin_info: Default::default(),
        }
    }

    /// The `return` token
    pub fn token(&self) -> &TokenReference {
        &self.token
    }

    /// The values being returned
    pub fn returns(&self) -> &Punctuated<Expression<P>> {
        &self.returns
    }

    /// Returns a new Return with the given `return` token
    pub fn with_token(self, token: TokenReference) -> Self {
        Self { token, ..self }
    }

    /// Returns a new Return with the given punctuated sequence
    pub fn with_returns(self, returns: Punctuated<Expression<P>>) -> Self {
        Self { returns, ..self }
    }
}

impl Default for Return {
    fn default() -> Self {
        Self::new()
    }
}

/// Fields of a [`TableConstructor`]
#[derive(Clone, Debug, Display, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Field<P: Plugin = DefaultPlugin> {
    /// A key in the format of `[expression] = value`
    #[display(
        fmt = "{}{}{}{}{}",
        "brackets.tokens().0",
        "key",
        "brackets.tokens().1",
        "equal",
        "value"
    )]
    ExpressionKey {
        /// The `[...]` part of `[expression] = value`
        brackets: ContainedSpan,
        /// The `expression` part of `[expression] = value`
        key: Expression<P>,
        /// The `=` part of `[expression] = value`
        equal: TokenReference,
        /// The `value` part of `[expression] = value`
        value: Expression<P>,
    },

    /// A key in the format of `name = value`
    #[display(fmt = "{}{}{}", "key", "equal", "value")]
    NameKey {
        /// The `name` part of `name = value`
        key: TokenReference,
        /// The `=` part of `name = value`
        equal: TokenReference,
        /// The `value` part of `name = value`
        value: Expression<P>,
    },

    /// A field with no key, just a value (such as `"a"` in `{ "a" }`)
    #[display(fmt = "{}", "_0")]
    NoKey(Expression<P>),

    #[display(fmt = "{}", "display_plugin_info!(Field<P>, self)")]
    Plugin(<<P as Plugin>::FieldMod as PluginMod<Field<P>>>::NodeInfo),
}

/// A table being constructed, such as `{ 1, 2, 3 }` or `{ a = 1 }`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}{}", "braces.tokens().0", "fields", "braces.tokens().1")]
pub struct TableConstructor<P: Plugin = DefaultPlugin> {
    #[node(full_range)]
    #[visit(contains = "fields")]
    braces: ContainedSpan,
    fields: Punctuated<Field<P>>,
}

impl<P: Plugin> TableConstructor<P> {
    /// Creates a new empty TableConstructor
    /// Brace tokens are followed by spaces, such that { `fields` }
    pub fn new() -> Self {
        Self {
            braces: ContainedSpan::new(
                TokenReference::symbol("{ ").unwrap(),
                TokenReference::symbol(" }").unwrap(),
            ),
            fields: Punctuated::new(),
        }
    }

    /// The braces of the constructor
    pub fn braces(&self) -> &ContainedSpan {
        &self.braces
    }

    /// Returns the [`Punctuated`] sequence of the fields used to create the table
    pub fn fields(&self) -> &Punctuated<Field<P>> {
        &self.fields
    }

    /// Returns a new TableConstructor with the given braces
    pub fn with_braces(self, braces: ContainedSpan) -> Self {
        Self { braces, ..self }
    }

    /// Returns a new TableConstructor with the given fields
    pub fn with_fields(self, fields: Punctuated<Field<P>>) -> Self {
        Self { fields, ..self }
    }
}

impl Default for TableConstructor {
    fn default() -> Self {
        Self::new()
    }
}

/// An expression, mostly useful for getting values
#[derive(Clone, Debug, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
#[non_exhaustive]
pub enum Expression<P: Plugin = DefaultPlugin> {
    /// A binary operation, such as `1 + 3`
    BinaryOperator {
        /// The left hand side of the binary operation, the `1` part of `1 + 3`
        lhs: Box<Expression<P>>,
        /// The binary operation used, the `+` part of `1 + 3`
        binop: BinOp,
        /// The right hand side of the binary operation, the `3` part of `1 + 3`
        rhs: Box<Expression<P>>,
    },

    /// A statement in parentheses, such as `(#list)`
    Parentheses {
        /// The parentheses of the `ParenExpression`
        #[node(full_range)]
        contained: ContainedSpan,
        /// The expression inside the parentheses
        expression: Box<Expression<P>>,
    },

    /// A unary operation, such as `#list`
    UnaryOperator {
        /// The unary operation, the `#` part of `#list`
        unop: UnOp,
        /// The expression the operation is being done on, the `list` part of `#list`
        expression: Box<Expression<P>>,
    },

    /// A value, such as "strings"
    Value {
        /// The value itself
        value: Box<Value<P>>,
        /// What the value is being asserted as using `::`.
        /// Only available when the "roblox" feature flag is enabled.
        #[cfg(feature = "roblox")]
        #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
        #[visit(skip)]
        type_assertion: Option<TypeAssertion>,
    },

    Plugin(<<P as Plugin>::ExpressionMod as PluginMod<Expression<P>>>::NodeInfo),
}

impl<P: Plugin> fmt::Display for Expression<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::BinaryOperator { lhs, binop, rhs } => write!(f, "{}{}{}", lhs, binop, rhs),

            Expression::Parentheses {
                contained,
                expression,
            } => write!(
                f,
                "{}{}{}",
                contained.tokens().0,
                expression,
                contained.tokens().1
            ),

            Expression::UnaryOperator { unop, expression } => write!(f, "{}{}", unop, expression),

            Expression::Value {
                value,
                #[cfg(feature = "roblox")]
                type_assertion,
            } => {
                write!(f, "{}", value)?;

                #[cfg(feature = "roblox")]
                if let Some(type_assertion) = type_assertion {
                    write!(f, "{}", type_assertion)?;
                }

                Ok(())
            }

            Expression::Plugin(_) => write!(f, "{}", display_plugin_info!(Expression<P>, self)),
        }
    }
}

/// Values that cannot be used standalone, but as part of things such as [`Stmt`]
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Value<P: Plugin = DefaultPlugin> {
    /// An anonymous function, such as `function() end)`
    #[display(fmt = "{}{}", "_0.0", "_0.1")]
    Function((TokenReference, FunctionBody<P>)),
    /// A call of a function, such as `call()`
    #[display(fmt = "{}", "_0")]
    FunctionCall(FunctionCall<P>),
    /// An if expression, such as `if foo then true else false`.
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    #[display(fmt = "{}", "_0")]
    IfExpression(IfExpression),
    /// A table constructor, such as `{ 1, 2, 3 }`
    #[display(fmt = "{}", "_0")]
    TableConstructor(TableConstructor<P>),
    /// A number token, such as `3.3`
    #[display(fmt = "{}", "_0")]
    Number(TokenReference),
    /// An expression between parentheses, such as `(3 + 2)`
    #[display(fmt = "{}", "_0")]
    ParenthesesExpression(Expression<P>),
    /// A string token, such as `"hello"`
    #[display(fmt = "{}", "_0")]
    String(TokenReference),
    /// A symbol, such as `true`
    #[display(fmt = "{}", "_0")]
    Symbol(TokenReference),
    /// A more complex value, such as `call().x`
    #[display(fmt = "{}", "_0")]
    Var(Var<P>),
    #[display(fmt = "{}", "display_plugin_info!(Value<P>, self)")]
    Plugin(<<P as Plugin>::ValueMod as PluginMod<Value<P>>>::NodeInfo),
}

/// A statement that stands alone
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Stmt<P: Plugin = DefaultPlugin> {
    /// An assignment, such as `x = 1`
    #[display(fmt = "{}", _0)]
    Assignment(Assignment<P>),
    /// A do block, `do end`
    #[display(fmt = "{}", _0)]
    Do(Do<P>),
    /// A function call on its own, such as `call()`
    #[display(fmt = "{}", _0)]
    FunctionCall(FunctionCall<P>),
    /// A function declaration, such as `function x() end`
    #[display(fmt = "{}", _0)]
    FunctionDeclaration(FunctionDeclaration<P>),
    /// A generic for loop, such as `for index, value in pairs(list) do end`
    #[display(fmt = "{}", _0)]
    GenericFor(GenericFor<P>),
    /// An if statement
    #[display(fmt = "{}", _0)]
    If(If<P>),
    /// A local assignment, such as `local x = 1`
    #[display(fmt = "{}", _0)]
    LocalAssignment(LocalAssignment<P>),
    /// A local function declaration, such as `local function x() end`
    #[display(fmt = "{}", _0)]
    LocalFunction(LocalFunction<P>),
    /// A numeric for loop, such as `for index = 1, 10 do end`
    #[display(fmt = "{}", _0)]
    NumericFor(NumericFor<P>),
    /// A repeat loop
    #[display(fmt = "{}", _0)]
    Repeat(Repeat<P>),
    /// A while loop
    #[display(fmt = "{}", _0)]
    While(While<P>),

    /// A compound assignment, such as `+=`
    /// Only available when the "roblox" feature flag is enabled
    #[cfg(feature = "roblox")]
    #[display(fmt = "{}", _0)]
    CompoundAssignment(CompoundAssignment),
    /// An exported type declaration, such as `export type Meters = number`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    ExportedTypeDeclaration(ExportedTypeDeclaration),
    /// A type declaration, such as `type Meters = number`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    TypeDeclaration(TypeDeclaration),

    /// A goto statement, such as `goto label`
    /// Only available when the "lua52" feature flag is enabled.
    #[cfg(feature = "lua52")]
    Goto(Goto),
    /// A label, such as `::label::`
    /// Only available when the "lua52" feature flag is enabled.
    #[cfg(feature = "lua52")]
    Label(Label),

    #[display(fmt = "{}", "display_plugin_info!(Stmt<P>, self)")]
    Plugin(<<P as Plugin>::StmtMod as PluginMod<Stmt<P>>>::NodeInfo),
}

/// A node used before another in cases such as function calling
/// The `("foo")` part of `("foo"):upper()`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Prefix<P: Plugin = DefaultPlugin> {
    /// A complicated expression, such as `("foo")`
    #[display(fmt = "{}", "_0")]
    Expression(Expression<P>),

    /// Just a name, such as `foo`
    #[display(fmt = "{}", "_0")]
    Name(TokenReference),

    #[display(fmt = "{}", "display_plugin_info!(Prefix<P>, self)")]
    Plugin(<<P as Plugin>::PrefixMod as PluginMod<Prefix<P>>>::NodeInfo),
}

/// The indexing of something, such as `x.y` or `x["y"]`
/// Values of variants are the keys, such as `"y"`
#[derive(Clone, Debug, Display, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Index<P: Plugin = DefaultPlugin> {
    /// Indexing in the form of `x["y"]`
    #[display(
        fmt = "{}{}{}",
        "brackets.tokens().0",
        "expression",
        "brackets.tokens().1"
    )]
    Brackets {
        /// The `[...]` part of `["y"]`
        brackets: ContainedSpan,
        /// The `"y"` part of `["y"]`
        expression: Expression<P>,
    },

    /// Indexing in the form of `x.y`
    #[display(fmt = "{}{}", "dot", "name")]
    Dot {
        /// The `.` part of `.y`
        dot: TokenReference,
        /// The `y` part of `.y`
        name: TokenReference,
    },

    #[display(fmt = "{}", "display_plugin_info!(Index<P>, self)")]
    Plugin(<<P as Plugin>::IndexMod as PluginMod<Index<P>>>::NodeInfo),
}

/// Arguments used for a function
#[derive(Clone, Debug, Display, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum FunctionArgs<P: Plugin = DefaultPlugin> {
    /// Used when a function is called in the form of `call(1, 2, 3)`
    #[display(
        fmt = "{}{}{}",
        "parentheses.tokens().0",
        "arguments",
        "parentheses.tokens().1"
    )]
    Parentheses {
        /// The `(...) part of (1, 2, 3)`
        #[node(full_range)]
        parentheses: ContainedSpan,
        /// The `1, 2, 3` part of `1, 2, 3`
        arguments: Punctuated<Expression<P>>,
    },
    /// Used when a function is called in the form of `call "foobar"`
    #[display(fmt = "{}", "_0")]
    String(TokenReference),
    /// Used when a function is called in the form of `call { 1, 2, 3 }`
    #[display(fmt = "{}", "_0")]
    TableConstructor(TableConstructor<P>),

    #[display(fmt = "{}", "display_plugin_info!(FunctionArgs<P>, self)")]
    Plugin(<<P as Plugin>::FunctionArgsMod as PluginMod<FunctionArgs<P>>>::NodeInfo),
}

/// A numeric for loop, such as `for index = 1, 10 do end`
#[derive(Clone, Debug, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct NumericFor<P: Plugin = DefaultPlugin> {
    for_token: TokenReference,
    index_variable: TokenReference,
    equal_token: TokenReference,
    start: Expression<P>,
    start_end_comma: TokenReference,
    end: Expression<P>,
    end_step_comma: Option<TokenReference>,
    step: Option<Expression<P>>,
    do_token: TokenReference,
    block: Block<P>,
    end_token: TokenReference,
    #[cfg(feature = "roblox")]
    type_specifier: Option<TypeSpecifier>,
    plugin_info: <<P as Plugin>::NumericForMod as PluginMod<NumericFor<P>>>::NodeInfo,
}

impl<P: Plugin> NumericFor<P> {
    /// Creates a new NumericFor from the given index variable, start, and end expressions
    pub fn new(index_variable: TokenReference, start: Expression<P>, end: Expression<P>) -> Self {
        Self {
            for_token: TokenReference::symbol("for ").unwrap(),
            index_variable,
            equal_token: TokenReference::symbol(" = ").unwrap(),
            start,
            start_end_comma: TokenReference::symbol(", ").unwrap(),
            end,
            end_step_comma: None,
            step: None,
            do_token: TokenReference::symbol(" do\n").unwrap(),
            block: Block::new(),
            end_token: TokenReference::symbol("\nend").unwrap(),
            plugin_info: Default::default(),
            #[cfg(feature = "roblox")]
            type_specifier: None,
        }
    }

    /// The `for` token
    pub fn for_token(&self) -> &TokenReference {
        &self.for_token
    }

    /// The index identity, `index` in the initial example
    pub fn index_variable(&self) -> &TokenReference {
        &self.index_variable
    }

    /// The `=` token
    pub fn equal_token(&self) -> &TokenReference {
        &self.equal_token
    }

    /// The starting point, `1` in the initial example
    pub fn start(&self) -> &Expression<P> {
        &self.start
    }

    /// The comma in between the starting point and end point
    /// for _ = 1, 10 do
    ///          ^
    pub fn start_end_comma(&self) -> &TokenReference {
        &self.start_end_comma
    }

    /// The ending point, `10` in the initial example
    pub fn end(&self) -> &Expression<P> {
        &self.end
    }

    /// The comma in between the ending point and limit, if one exists
    /// for _ = 0, 10, 2 do
    ///              ^
    pub fn end_step_comma(&self) -> Option<&TokenReference> {
        self.end_step_comma.as_ref()
    }

    /// The step if one exists, `2` in `for index = 0, 10, 2 do end`
    pub fn step(&self) -> Option<&Expression<P>> {
        self.step.as_ref()
    }

    /// The `do` token
    pub fn do_token(&self) -> &TokenReference {
        &self.do_token
    }

    /// The code inside the for loop
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// The type specifiers of the index variable
    /// `for i: number = 1, 10 do` returns:
    /// `Some(TypeSpecifier(number))`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn type_specifier(&self) -> Option<&TypeSpecifier> {
        self.type_specifier.as_ref()
    }

    /// Returns a new NumericFor with the given for token
    pub fn with_for_token(self, for_token: TokenReference) -> Self {
        Self { for_token, ..self }
    }

    /// Returns a new NumericFor with the given index variable
    pub fn with_index_variable(self, index_variable: TokenReference) -> Self {
        Self {
            index_variable,
            ..self
        }
    }

    /// Returns a new NumericFor with the given `=` token
    pub fn with_equal_token(self, equal_token: TokenReference) -> Self {
        Self {
            equal_token,
            ..self
        }
    }

    /// Returns a new NumericFor with the given start expression
    pub fn with_start(self, start: Expression<P>) -> Self {
        Self { start, ..self }
    }

    /// Returns a new NumericFor with the given comma between the start and end expressions
    pub fn with_start_end_comma(self, start_end_comma: TokenReference) -> Self {
        Self {
            start_end_comma,
            ..self
        }
    }

    /// Returns a new NumericFor with the given end expression
    pub fn with_end(self, end: Expression<P>) -> Self {
        Self { end, ..self }
    }

    /// Returns a new NumericFor with the given comma between the end and the step expressions
    pub fn with_end_step_comma(self, end_step_comma: Option<TokenReference>) -> Self {
        Self {
            end_step_comma,
            ..self
        }
    }

    /// Returns a new NumericFor with the given step expression
    pub fn with_step(self, step: Option<Expression<P>>) -> Self {
        Self { step, ..self }
    }

    /// Returns a new NumericFor with the given `do` token
    pub fn with_do_token(self, do_token: TokenReference) -> Self {
        Self { do_token, ..self }
    }

    /// Returns a new NumericFor with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new NumericFor with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }

    /// Returns a new NumericFor with the given type specifiers
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn with_type_specifier(self, type_specifier: Option<TypeSpecifier>) -> Self {
        Self {
            type_specifier,
            ..self
        }
    }
}

impl<P: Plugin> fmt::Display for NumericFor<P> {
    #[cfg(feature = "roblox")]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}{}{}{}{}{}{}{}",
            self.for_token,
            self.index_variable,
            display_option(self.type_specifier()),
            self.equal_token,
            self.start,
            self.start_end_comma,
            self.end,
            display_option(self.end_step_comma()),
            display_option(self.step()),
            self.do_token,
            self.block,
            self.end_token,
        )
    }

    #[cfg(not(feature = "roblox"))]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}{}{}{}{}{}{}",
            self.for_token,
            self.index_variable,
            self.equal_token,
            self.start,
            self.start_end_comma,
            self.end,
            display_option(self.end_step_comma()),
            display_option(self.step()),
            self.do_token,
            self.block,
            self.end_token,
        )
    }
}

/// A generic for loop, such as `for index, value in pairs(list) do end`
#[derive(Clone, Debug, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct GenericFor<P: Plugin = DefaultPlugin> {
    for_token: TokenReference,
    names: Punctuated<TokenReference>,
    in_token: TokenReference,
    expr_list: Punctuated<Expression<P>>,
    do_token: TokenReference,
    block: Block<P>,
    end_token: TokenReference,
    #[cfg(feature = "roblox")]
    type_specifiers: Vec<Option<TypeSpecifier>>,
    plugin_info: <<P as Plugin>::GenericForMod as PluginMod<GenericFor<P>>>::NodeInfo,
}

impl<P: Plugin> GenericFor<P> {
    /// Creates a new GenericFor from the given names and expressions
    pub fn new(names: Punctuated<TokenReference>, expr_list: Punctuated<Expression<P>>) -> Self {
        Self {
            for_token: TokenReference::symbol("for ").unwrap(),
            names,
            in_token: TokenReference::symbol(" in ").unwrap(),
            expr_list,
            do_token: TokenReference::symbol(" do\n").unwrap(),
            block: Block::new(),
            end_token: TokenReference::symbol("\nend").unwrap(),
            plugin_info: Default::default(),
            #[cfg(feature = "roblox")]
            type_specifiers: Vec::new(),
        }
    }

    /// The `for` token
    pub fn for_token(&self) -> &TokenReference {
        &self.for_token
    }

    /// Returns the punctuated sequence of names
    /// In `for index, value in pairs(list) do`, iterates over `index` and `value`
    pub fn names(&self) -> &Punctuated<TokenReference> {
        &self.names
    }

    /// The `in` token
    pub fn in_token(&self) -> &TokenReference {
        &self.in_token
    }

    /// Returns the punctuated sequence of the expressions looped over
    /// In `for index, value in pairs(list) do`, iterates over `pairs(list)`
    pub fn expressions(&self) -> &Punctuated<Expression<P>> {
        &self.expr_list
    }

    /// The `do` token
    pub fn do_token(&self) -> &TokenReference {
        &self.do_token
    }

    /// The code inside the for loop
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// The type specifiers of the named variables, in the order that they were assigned.
    /// `for i, v: string in pairs() do` returns an iterator containing:
    /// `None, Some(TypeSpecifier(string))`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn type_specifiers(&self) -> impl Iterator<Item = Option<&TypeSpecifier>> {
        self.type_specifiers.iter().map(Option::as_ref)
    }

    /// Returns a new GenericFor with the given `for` token
    pub fn with_for_token(self, for_token: TokenReference) -> Self {
        Self { for_token, ..self }
    }

    /// Returns a new GenericFor with the given names
    pub fn with_names(self, names: Punctuated<TokenReference>) -> Self {
        Self { names, ..self }
    }

    /// Returns a new GenericFor with the given `in` token
    pub fn with_in_token(self, in_token: TokenReference) -> Self {
        Self { in_token, ..self }
    }

    /// Returns a new GenericFor with the given expression list
    pub fn with_expressions(self, expr_list: Punctuated<Expression<P>>) -> Self {
        Self { expr_list, ..self }
    }

    /// Returns a new GenericFor with the given `do` token
    pub fn with_do_token(self, do_token: TokenReference) -> Self {
        Self { do_token, ..self }
    }

    /// Returns a new GenericFor with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new GenericFor with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }

    /// Returns a new GenericFor with the given type specifiers
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn with_type_specifiers(self, type_specifiers: Vec<Option<TypeSpecifier>>) -> Self {
        Self {
            type_specifiers,
            ..self
        }
    }
}

impl<P: Plugin> fmt::Display for GenericFor<P> {
    #[cfg(feature = "roblox")]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}{}{}",
            self.for_token,
            join_type_specifiers(&self.names, self.type_specifiers()),
            self.in_token,
            self.expr_list,
            self.do_token,
            self.block,
            self.end_token
        )
    }

    #[cfg(not(feature = "roblox"))]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}{}{}",
            self.for_token,
            self.names,
            self.in_token,
            self.expr_list,
            self.do_token,
            self.block,
            self.end_token
        )
    }
}

/// An if statement
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(
    fmt = "{}{}{}{}{}{}{}{}",
    "if_token",
    "condition",
    "then_token",
    "block",
    "display_option(else_if.as_ref().map(join_vec))",
    "display_option(else_token)",
    "display_option(r#else)",
    "end_token"
)]
pub struct If<P: Plugin = DefaultPlugin> {
    if_token: TokenReference,
    condition: Expression<P>,
    then_token: TokenReference,
    block: Block<P>,
    else_if: Option<Vec<ElseIf<P>>>,
    else_token: Option<TokenReference>,
    #[cfg_attr(feature = "serde", serde(rename = "else"))]
    r#else: Option<Block<P>>,
    end_token: TokenReference,
    plugin_info: <<P as Plugin>::IfMod as PluginMod<If<P>>>::NodeInfo,
}

impl<P: Plugin> If<P> {
    /// Creates a new If from the given condition
    pub fn new(condition: Expression<P>) -> Self {
        Self {
            if_token: TokenReference::symbol("if ").unwrap(),
            condition,
            then_token: TokenReference::symbol(" then").unwrap(),
            block: Block::new(),
            else_if: None,
            else_token: None,
            r#else: None,
            end_token: TokenReference::symbol("\nend").unwrap(),
            plugin_info: Default::default(),
        }
    }

    /// The `if` token
    pub fn if_token(&self) -> &TokenReference {
        &self.if_token
    }

    /// The condition of the if statement, `condition` in `if condition then`
    pub fn condition(&self) -> &Expression<P> {
        &self.condition
    }

    /// The `then` token
    pub fn then_token(&self) -> &TokenReference {
        &self.then_token
    }

    /// The block inside the initial if statement
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `else` token if one exists
    pub fn else_token(&self) -> Option<&TokenReference> {
        self.else_token.as_ref()
    }

    /// If there are `elseif` conditions, returns a vector of them
    /// Expression is the condition, block is the code if the condition is true
    // TODO: Make this return an iterator, and remove Option part entirely?
    pub fn else_if(&self) -> Option<&Vec<ElseIf<P>>> {
        self.else_if.as_ref()
    }

    /// The code inside an `else` block if one exists
    pub fn else_block(&self) -> Option<&Block<P>> {
        self.r#else.as_ref()
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// Returns a new If with the given `if` token
    pub fn with_if_token(self, if_token: TokenReference) -> Self {
        Self { if_token, ..self }
    }

    /// Returns a new If with the given condition
    pub fn with_condition(self, condition: Expression<P>) -> Self {
        Self { condition, ..self }
    }

    /// Returns a new If with the given `then` token
    pub fn with_then_token(self, then_token: TokenReference) -> Self {
        Self { then_token, ..self }
    }

    /// Returns a new If with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new If with the given list of `elseif` blocks
    pub fn with_else_if(self, else_if: Option<Vec<ElseIf<P>>>) -> Self {
        Self { else_if, ..self }
    }

    /// Returns a new If with the given `else` token
    pub fn with_else_token(self, else_token: Option<TokenReference>) -> Self {
        Self { else_token, ..self }
    }

    /// Returns a new If with the given `else` body
    pub fn with_else(self, r#else: Option<Block<P>>) -> Self {
        Self { r#else, ..self }
    }

    /// Returns a new If with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }
}

/// An elseif block in a bigger [`If`] statement
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(bounds = "P: Plugin")]
#[display(fmt = "{}{}{}{}", "else_if_token", "condition", "then_token", "block")]
pub struct ElseIf<P: Plugin = DefaultPlugin> {
    else_if_token: TokenReference,
    condition: Expression<P>,
    then_token: TokenReference,
    block: Block<P>,
    plugin_info: <<P as Plugin>::ElseIfMod as PluginMod<ElseIf<P>>>::NodeInfo,
}

impl<P: Plugin> ElseIf<P> {
    /// Creates a new ElseIf from the given condition
    pub fn new(condition: Expression<P>) -> Self {
        Self {
            else_if_token: TokenReference::symbol("elseif ").unwrap(),
            condition,
            then_token: TokenReference::symbol(" then\n").unwrap(),
            block: Block::new(),
            plugin_info: Default::default(),
        }
    }

    /// The `elseif` token
    pub fn else_if_token(&self) -> &TokenReference {
        &self.else_if_token
    }

    /// The condition of the `elseif`, `condition` in `elseif condition then`
    pub fn condition(&self) -> &Expression<P> {
        &self.condition
    }

    /// The `then` token
    pub fn then_token(&self) -> &TokenReference {
        &self.then_token
    }

    /// The body of the `elseif`
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// Returns a new ElseIf with the given `elseif` token
    pub fn with_else_if_token(self, else_if_token: TokenReference) -> Self {
        Self {
            else_if_token,
            ..self
        }
    }

    /// Returns a new ElseIf with the given condition
    pub fn with_condition(self, condition: Expression<P>) -> Self {
        Self { condition, ..self }
    }

    /// Returns a new ElseIf with the given `then` token
    pub fn with_then_token(self, then_token: TokenReference) -> Self {
        Self { then_token, ..self }
    }

    /// Returns a new ElseIf with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }
}

/// A while loop
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(
    fmt = "{}{}{}{}{}",
    "while_token",
    "condition",
    "do_token",
    "block",
    "end_token"
)]
pub struct While<P: Plugin = DefaultPlugin> {
    while_token: TokenReference,
    condition: Expression<P>,
    do_token: TokenReference,
    block: Block<P>,
    end_token: TokenReference,
    plugin_info: <<P as Plugin>::WhileMod as PluginMod<While<P>>>::NodeInfo,
}

impl<P: Plugin> While<P> {
    /// Creates a new While from the given condition
    pub fn new(condition: Expression<P>) -> Self {
        Self {
            while_token: TokenReference::symbol("while ").unwrap(),
            condition,
            do_token: TokenReference::symbol(" do\n").unwrap(),
            block: Block::new(),
            end_token: TokenReference::symbol("end\n").unwrap(),
            plugin_info: Default::default(),
        }
    }

    /// The `while` token
    pub fn while_token(&self) -> &TokenReference {
        &self.while_token
    }

    /// The `condition` part of `while condition do`
    pub fn condition(&self) -> &Expression<P> {
        &self.condition
    }

    /// The `do` token
    pub fn do_token(&self) -> &TokenReference {
        &self.do_token
    }

    /// The code inside the while loop
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// Returns a new While with the given `while` token
    pub fn with_while_token(self, while_token: TokenReference) -> Self {
        Self {
            while_token,
            ..self
        }
    }

    /// Returns a new While with the given condition
    pub fn with_condition(self, condition: Expression<P>) -> Self {
        Self { condition, ..self }
    }

    /// Returns a new While with the given `do` token
    pub fn with_do_token(self, do_token: TokenReference) -> Self {
        Self { do_token, ..self }
    }

    /// Returns a new While with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new While with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }
}

/// A repeat loop
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}{}{}", "repeat_token", "block", "until_token", "until")]
pub struct Repeat<P: Plugin = DefaultPlugin> {
    repeat_token: TokenReference,
    block: Block<P>,
    until_token: TokenReference,
    until: Expression<P>,
    plugin_info: <<P as Plugin>::RepeatMod as PluginMod<Repeat<P>>>::NodeInfo,
}

impl<P: Plugin> Repeat<P> {
    /// Creates a new Repeat from the given expression to repeat until
    pub fn new(until: Expression<P>) -> Self {
        Self {
            repeat_token: TokenReference::symbol("repeat\n").unwrap(),
            block: Block::new(),
            until_token: TokenReference::symbol("\nuntil ").unwrap(),
            until,
            plugin_info: Default::default(),
        }
    }

    /// The `repeat` token
    pub fn repeat_token(&self) -> &TokenReference {
        &self.repeat_token
    }

    /// The code inside the `repeat` block
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `until` token
    pub fn until_token(&self) -> &TokenReference {
        &self.until_token
    }

    /// The condition for the `until` part
    pub fn until(&self) -> &Expression<P> {
        &self.until
    }

    /// Returns a new Repeat with the given `repeat` token
    pub fn with_repeat_token(self, repeat_token: TokenReference) -> Self {
        Self {
            repeat_token,
            ..self
        }
    }

    /// Returns a new Repeat with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new Repeat with the given `until` token
    pub fn with_until_token(self, until_token: TokenReference) -> Self {
        Self {
            until_token,
            ..self
        }
    }

    /// Returns a new Repeat with the given `until` block
    pub fn with_until(self, until: Expression<P>) -> Self {
        Self { until, ..self }
    }
}

/// A method call, such as `x:y()`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}{}", "colon_token", "name", "args")]
pub struct MethodCall<P: Plugin = DefaultPlugin> {
    colon_token: TokenReference,
    name: TokenReference,
    args: FunctionArgs<P>,
    plugin_info: <<P as Plugin>::MethodCallMod as PluginMod<MethodCall<P>>>::NodeInfo,
}

impl<P: Plugin> MethodCall<P> {
    /// Returns a new MethodCall from the given name and args
    pub fn new(name: TokenReference, args: FunctionArgs<P>) -> Self {
        Self {
            colon_token: TokenReference::symbol(":").unwrap(),
            name,
            args,
            plugin_info: Default::default(),
        }
    }

    /// The `:` in `x:y()`
    pub fn colon_token(&self) -> &TokenReference {
        &self.colon_token
    }

    /// The arguments of a method call, the `x, y, z` part of `method:call(x, y, z)`
    pub fn args(&self) -> &FunctionArgs<P> {
        &self.args
    }

    /// The method being called, the `call` part of `method:call()`
    pub fn name(&self) -> &TokenReference {
        &self.name
    }

    /// Returns a new MethodCall with the given `:` token
    pub fn with_colon_token(self, colon_token: TokenReference) -> Self {
        Self {
            colon_token,
            ..self
        }
    }

    /// Returns a new MethodCall with the given name
    pub fn with_name(self, name: TokenReference) -> Self {
        Self { name, ..self }
    }

    /// Returns a new MethodCall with the given args
    pub fn with_args(self, args: FunctionArgs<P>) -> Self {
        Self { args, ..self }
    }
}

/// Something being called
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Call<P: Plugin = DefaultPlugin> {
    #[display(fmt = "{}", "_0")]
    /// A function being called directly, such as `x(1)`
    AnonymousCall(FunctionArgs<P>),
    #[display(fmt = "{}", "_0")]
    /// A method call, such as `x:y()`
    MethodCall(MethodCall<P>),

    #[display(fmt = "{}", "display_plugin_info!(Call<P>, self)")]
    Plugin(<<P as Plugin>::CallMod as PluginMod<Call<P>>>::NodeInfo),
}

/// A function body, everything except `function x` in `function x(a, b, c) call() end`
#[derive(Clone, Debug, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct FunctionBody<P: Plugin = DefaultPlugin> {
    #[cfg(feature = "roblox")]
    #[visit(skip)]
    generics: Option<GenericDeclaration>,

    parameters_parentheses: ContainedSpan,
    parameters: Punctuated<Parameter<P>>,

    #[cfg(feature = "roblox")]
    #[visit(skip)]
    type_specifiers: Vec<Option<TypeSpecifier>>,

    #[cfg(feature = "roblox")]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    #[visit(skip)]
    return_type: Option<TypeSpecifier>,

    block: Block<P>,
    end_token: TokenReference,

    plugin_info: <<P as Plugin>::FunctionBodyMod as PluginMod<FunctionBody<P>>>::NodeInfo,
}

impl<P: Plugin> FunctionBody<P> {
    /// Returns a new empty FunctionBody
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "roblox")]
            generics: None,

            parameters_parentheses: ContainedSpan::new(
                TokenReference::symbol("(").unwrap(),
                TokenReference::symbol(")").unwrap(),
            ),
            parameters: Punctuated::new(),

            #[cfg(feature = "roblox")]
            type_specifiers: Vec::new(),

            #[cfg(feature = "roblox")]
            return_type: None,

            block: Block::new(),
            end_token: TokenReference::symbol("\nend").unwrap(),

            plugin_info: Default::default(),
        }
    }

    /// The parentheses of the parameters
    pub fn parameters_parentheses(&self) -> &ContainedSpan {
        &self.parameters_parentheses
    }

    /// Returns the [`Punctuated`] sequence of the parameters for the function declaration
    pub fn parameters(&self) -> &Punctuated<Parameter<P>> {
        &self.parameters
    }

    /// The code of a function body
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// The generics declared for the function body.
    /// The `<T, U>` part of `function x<T, U>() end`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn generics(&self) -> Option<&GenericDeclaration> {
        self.generics.as_ref()
    }

    /// The type specifiers of the variables, in the order that they were assigned.
    /// `(foo: number, bar, baz: boolean)` returns an iterator containing:
    /// `Some(TypeSpecifier(number)), None, Some(TypeSpecifier(boolean))`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn type_specifiers(&self) -> impl Iterator<Item = Option<&TypeSpecifier>> {
        self.type_specifiers.iter().map(Option::as_ref)
    }

    /// The return type of the function, if one exists.
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn return_type(&self) -> Option<&TypeSpecifier> {
        self.return_type.as_ref()
    }

    /// Returns a new FunctionBody with the given parentheses for the parameters
    pub fn with_parameters_parentheses(self, parameters_parentheses: ContainedSpan) -> Self {
        Self {
            parameters_parentheses,
            ..self
        }
    }

    /// Returns a new FunctionBody with the given parameters
    pub fn with_parameters(self, parameters: Punctuated<Parameter<P>>) -> Self {
        Self { parameters, ..self }
    }

    /// Returns a new FunctionBody with the given generics declaration
    #[cfg(feature = "roblox")]
    pub fn with_generics(self, generics: Option<GenericDeclaration>) -> Self {
        Self { generics, ..self }
    }

    /// Returns a new FunctionBody with the given type specifiers
    #[cfg(feature = "roblox")]
    pub fn with_type_specifiers(self, type_specifiers: Vec<Option<TypeSpecifier>>) -> Self {
        Self {
            type_specifiers,
            ..self
        }
    }

    /// Returns a new FunctionBody with the given return type
    #[cfg(feature = "roblox")]
    pub fn with_return_type(self, return_type: Option<TypeSpecifier>) -> Self {
        Self {
            return_type,
            ..self
        }
    }

    /// Returns a new FunctionBody with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new FunctionBody with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }
}

impl Default for FunctionBody {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Plugin> fmt::Display for FunctionBody<P> {
    #[cfg(feature = "roblox")]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}{}{}",
            display_option(self.generics.as_ref()),
            self.parameters_parentheses.tokens().0,
            join_type_specifiers(&self.parameters, self.type_specifiers()),
            self.parameters_parentheses.tokens().1,
            display_option(self.return_type.as_ref()),
            self.block,
            self.end_token
        )
    }

    #[cfg(not(feature = "roblox"))]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}{}",
            self.parameters_parentheses.tokens().0,
            self.parameters,
            self.parameters_parentheses.tokens().1,
            self.block,
            self.end_token
        )
    }
}

/// A parameter in a function declaration
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Parameter<P: Plugin = DefaultPlugin> {
    /// The `...` vararg syntax, such as `function x(...)`
    Ellipse(TokenReference),
    /// A name parameter, such as `function x(a, b, c)`
    Name(TokenReference),

    #[display(fmt = "{}", "display_plugin_info!(Parameter<P>, self)")]
    Plugin(<<P as Plugin>::ParameterMod as PluginMod<Parameter<P>>>::NodeInfo),
}

/// A suffix in certain cases, such as `:y()` in `x:y()`
/// Can be stacked on top of each other, such as in `x()()()`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Suffix<P: Plugin = DefaultPlugin> {
    #[display(fmt = "{}", "_0")]
    /// A call, including method calls and direct calls
    Call(Call<P>),
    #[display(fmt = "{}", "_0")]
    /// An index, such as `x.y`
    Index(Index<P>),

    #[display(fmt = "{}", "display_plugin_info!(Suffix<P>, self)")]
    Plugin(<<P as Plugin>::SuffixMod as PluginMod<Suffix<P>>>::NodeInfo),
}

/// A complex expression used by [`Var`], consisting of both a prefix and suffixes
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[display(fmt = "{}{}", "prefix", "join_vec(suffixes)")]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct VarExpression<P: Plugin = DefaultPlugin> {
    prefix: Prefix<P>,
    suffixes: Vec<Suffix<P>>,
    plugin_info: <<P as Plugin>::VarExpressionMod as PluginMod<VarExpression<P>>>::NodeInfo,
}

impl<P: Plugin> VarExpression<P> {
    /// Returns a new VarExpression from the given prefix
    pub fn new(prefix: Prefix<P>) -> Self {
        Self {
            prefix,
            suffixes: Vec::new(),
            plugin_info: Default::default(),
        }
    }

    /// The prefix of the expression, such as a name
    pub fn prefix(&self) -> &Prefix<P> {
        &self.prefix
    }

    /// An iter over the suffixes, such as indexing or calling
    pub fn suffixes(&self) -> impl Iterator<Item = &Suffix<P>> {
        self.suffixes.iter()
    }

    /// Returns a new VarExpression with the given prefix
    pub fn with_prefix(self, prefix: Prefix<P>) -> Self {
        Self { prefix, ..self }
    }

    /// Returns a new VarExpression with the given suffixes
    pub fn with_suffixes(self, suffixes: Vec<Suffix<P>>) -> Self {
        Self { suffixes, ..self }
    }
}

/// Used in [`Assignment`s](Assignment) and [`Value`s](Value)
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[non_exhaustive]
pub enum Var<P: Plugin = DefaultPlugin> {
    /// An expression, such as `x.y.z` or `x()`
    #[display(fmt = "{}", "_0")]
    Expression(VarExpression<P>),
    /// A literal identifier, such as `x`
    #[display(fmt = "{}", "_0")]
    Name(TokenReference),

    #[display(fmt = "{}", "display_plugin_info!(Var<P>, self)")]
    Plugin(<<P as Plugin>::VarMod as PluginMod<Var<P>>>::NodeInfo),
}

/// An assignment, such as `x = y`. Not used for [`LocalAssignment`s](LocalAssignment)
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}{}", "var_list", "equal_token", "expr_list")]
pub struct Assignment<P: Plugin = DefaultPlugin> {
    var_list: Punctuated<Var<P>>,
    equal_token: TokenReference,
    expr_list: Punctuated<Expression<P>>,
    plugin_info: <<P as Plugin>::AssignmentMod as PluginMod<Assignment<P>>>::NodeInfo,
}

impl<P: Plugin> Assignment<P> {
    /// Returns a new Assignment from the given variable and expression list
    pub fn new(var_list: Punctuated<Var<P>>, expr_list: Punctuated<Expression<P>>) -> Self {
        Self {
            var_list,
            equal_token: TokenReference::symbol(" = ").unwrap(),
            expr_list,
            plugin_info: Default::default(),
        }
    }

    /// Returns the punctuated sequence over the expressions being assigned.
    /// This is the the `1, 2` part of `x, y["a"] = 1, 2`
    pub fn expressions(&self) -> &Punctuated<Expression<P>> {
        &self.expr_list
    }

    /// The `=` token in between `x = y`
    pub fn equal_token(&self) -> &TokenReference {
        &self.equal_token
    }

    /// Returns the punctuated sequence over the variables being assigned to.
    /// This is the `x, y["a"]` part of `x, y["a"] = 1, 2`
    pub fn variables(&self) -> &Punctuated<Var<P>> {
        &self.var_list
    }

    /// Returns a new Assignment with the given variables
    pub fn with_variables(self, var_list: Punctuated<Var<P>>) -> Self {
        Self { var_list, ..self }
    }

    /// Returns a new Assignment with the given `=` token
    pub fn with_equal_token(self, equal_token: TokenReference) -> Self {
        Self {
            equal_token,
            ..self
        }
    }

    /// Returns a new Assignment with the given expressions
    pub fn with_expressions(self, expr_list: Punctuated<Expression<P>>) -> Self {
        Self { expr_list, ..self }
    }
}

/// A declaration of a local function, such as `local function x() end`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(
    not(feature = "roblox"),
    display(fmt = "{}{}{}{}", "local_token", "function_token", "name", "body")
)]
#[cfg_attr(
    feature = "roblox",
    display(fmt = "{}{}{}{}", "local_token", "function_token", "name", "body")
)]
pub struct LocalFunction<P: Plugin = DefaultPlugin> {
    local_token: TokenReference,
    function_token: TokenReference,
    name: TokenReference,
    body: FunctionBody<P>,

    plugin_info: <<P as Plugin>::LocalFunctionMod as PluginMod<LocalFunction<P>>>::NodeInfo,
}

impl<P: Plugin> LocalFunction<P> {
    /// Returns a new LocalFunction from the given name
    pub fn new(name: TokenReference) -> Self {
        LocalFunction {
            local_token: TokenReference::symbol("local ").unwrap(),
            function_token: TokenReference::symbol("function ").unwrap(),
            name,
            body: FunctionBody::new(),
            plugin_info: Default::default(),
        }
    }

    /// The `local` token
    pub fn local_token(&self) -> &TokenReference {
        &self.local_token
    }

    /// The `function` token
    pub fn function_token(&self) -> &TokenReference {
        &self.function_token
    }

    /// The function body, everything except `local function x` in `local function x(a, b, c) call() end`
    pub fn body(&self) -> &FunctionBody<P> {
        &self.body
    }

    /// The name of the function, the `x` part of `local function x() end`
    pub fn name(&self) -> &TokenReference {
        &self.name
    }

    /// Returns a new LocalFunction with the given `local` token
    pub fn with_local_token(self, local_token: TokenReference) -> Self {
        Self {
            local_token,
            ..self
        }
    }

    /// Returns a new LocalFunction with the given `function` token
    pub fn with_function_token(self, function_token: TokenReference) -> Self {
        Self {
            function_token,
            ..self
        }
    }

    /// Returns a new LocalFunction with the given name
    pub fn with_name(self, name: TokenReference) -> Self {
        Self { name, ..self }
    }

    /// Returns a new LocalFunction with the given function body
    pub fn with_body(self, body: FunctionBody<P>) -> Self {
        Self { body, ..self }
    }
}

/// An assignment to a local variable, such as `local x = 1`
#[derive(Clone, Debug, PartialEq, Node)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct LocalAssignment<P: Plugin = DefaultPlugin> {
    local_token: TokenReference,
    #[cfg(feature = "roblox")]
    type_specifiers: Vec<Option<TypeSpecifier>>,
    name_list: Punctuated<TokenReference>,
    equal_token: Option<TokenReference>,
    expr_list: Punctuated<Expression<P>>,

    plugin_info: <<P as Plugin>::LocalAssignmentMod as PluginMod<LocalAssignment<P>>>::NodeInfo,
}

impl<P: Plugin> LocalAssignment<P> {
    /// Returns a new LocalAssignment from the given name list
    pub fn new(name_list: Punctuated<TokenReference>) -> Self {
        Self {
            local_token: TokenReference::symbol("local ").unwrap(),
            #[cfg(feature = "roblox")]
            type_specifiers: Vec::new(),
            name_list,
            equal_token: None,
            expr_list: Punctuated::new(),

            plugin_info: Default::default(),
        }
    }

    /// The `local` token
    pub fn local_token(&self) -> &TokenReference {
        &self.local_token
    }

    /// The `=` token in between `local x = y`, if one exists
    pub fn equal_token(&self) -> Option<&TokenReference> {
        self.equal_token.as_ref()
    }

    /// Returns the punctuated sequence of the expressions being assigned.
    /// This is the `1, 2` part of `local x, y = 1, 2`
    pub fn expressions(&self) -> &Punctuated<Expression<P>> {
        &self.expr_list
    }

    /// Returns the punctuated sequence of names being assigned to.
    /// This is the `x, y` part of `local x, y = 1, 2`
    pub fn names(&self) -> &Punctuated<TokenReference> {
        &self.name_list
    }

    /// The type specifiers of the variables, in the order that they were assigned.
    /// `local foo: number, bar, baz: boolean` returns an iterator containing:
    /// `Some(TypeSpecifier(number)), None, Some(TypeSpecifier(boolean))`
    /// Only available when the "roblox" feature flag is enabled.
    #[cfg(feature = "roblox")]
    pub fn type_specifiers(&self) -> impl Iterator<Item = Option<&TypeSpecifier>> {
        self.type_specifiers.iter().map(Option::as_ref)
    }

    /// Returns a new LocalAssignment with the given `local` token
    pub fn with_local_token(self, local_token: TokenReference) -> Self {
        Self {
            local_token,
            ..self
        }
    }

    /// Returns a new LocalAssignment with the given type specifiers
    #[cfg(feature = "roblox")]
    pub fn with_type_specifiers(self, type_specifiers: Vec<Option<TypeSpecifier>>) -> Self {
        Self {
            type_specifiers,
            ..self
        }
    }

    /// Returns a new LocalAssignment with the given name list
    pub fn with_names(self, name_list: Punctuated<TokenReference>) -> Self {
        Self { name_list, ..self }
    }

    /// Returns a new LocalAssignment with the given `=` token
    pub fn with_equal_token(self, equal_token: Option<TokenReference>) -> Self {
        Self {
            equal_token,
            ..self
        }
    }

    /// Returns a new LocalAssignment with the given expression list
    pub fn with_expressions(self, expr_list: Punctuated<Expression<P>>) -> Self {
        Self { expr_list, ..self }
    }
}

impl<P: Plugin> fmt::Display for LocalAssignment<P> {
    #[cfg(feature = "roblox")]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}",
            self.local_token,
            join_type_specifiers(&self.name_list, self.type_specifiers()),
            display_option(&self.equal_token),
            self.expr_list
        )
    }

    #[cfg(not(feature = "roblox"))]
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}{}{}",
            self.local_token,
            self.name_list,
            display_option(&self.equal_token),
            self.expr_list
        )
    }
}

/// A `do` block, such as `do ... end`
/// This is not used for things like `while true do end`, only those on their own
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}{}", "do_token", "block", "end_token")]
pub struct Do<P: Plugin = DefaultPlugin> {
    do_token: TokenReference,
    block: Block<P>,
    end_token: TokenReference,

    plugin_info: <<P as Plugin>::DoMod as PluginMod<Do<P>>>::NodeInfo,
}

impl<P: Plugin> Do<P> {
    /// Creates an empty Do
    pub fn new() -> Self {
        Self {
            do_token: TokenReference::symbol("do\n").unwrap(),
            block: Block::new(),
            end_token: TokenReference::symbol("\nend").unwrap(),
            plugin_info: Default::default(),
        }
    }

    /// The `do` token
    pub fn do_token(&self) -> &TokenReference {
        &self.do_token
    }

    /// The code inside the `do ... end`
    pub fn block(&self) -> &Block<P> {
        &self.block
    }

    /// The `end` token
    pub fn end_token(&self) -> &TokenReference {
        &self.end_token
    }

    /// Returns a new Do with the given `do` token
    pub fn with_do_token(self, do_token: TokenReference) -> Self {
        Self { do_token, ..self }
    }

    /// Returns a new Do with the given block
    pub fn with_block(self, block: Block<P>) -> Self {
        Self { block, ..self }
    }

    /// Returns a new Do with the given `end` token
    pub fn with_end_token(self, end_token: TokenReference) -> Self {
        Self { end_token, ..self }
    }
}

impl Default for Do {
    fn default() -> Self {
        Self::new()
    }
}

/// A function being called, such as `call()`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(fmt = "{}{}", "prefix", "join_vec(suffixes)")]
pub struct FunctionCall<P: Plugin = DefaultPlugin> {
    prefix: Prefix<P>,
    suffixes: Vec<Suffix<P>>,

    plugin_info: <<P as Plugin>::FunctionCallMod as PluginMod<FunctionCall<P>>>::NodeInfo,
}

impl<P: Plugin> FunctionCall<P> {
    /// Creates a new FunctionCall from the given prefix
    /// Sets the suffixes such that the return is `prefixes()`
    pub fn new(prefix: Prefix<P>) -> Self {
        FunctionCall {
            prefix,
            suffixes: vec![Suffix::Call(Call::AnonymousCall(
                FunctionArgs::Parentheses {
                    arguments: Punctuated::new(),
                    parentheses: ContainedSpan::new(
                        TokenReference::symbol("(").unwrap(),
                        TokenReference::symbol(")").unwrap(),
                    ),
                },
            ))],

            plugin_info: Default::default(),
        }
    }

    /// The prefix of a function call, the `call` part of `call()`
    pub fn prefix(&self) -> &Prefix<P> {
        &self.prefix
    }

    /// The suffix of a function call, the `()` part of `call()`
    pub fn suffixes(&self) -> impl Iterator<Item = &Suffix<P>> {
        self.suffixes.iter()
    }

    /// Returns a new FunctionCall with the given prefix
    pub fn with_prefix(self, prefix: Prefix<P>) -> Self {
        Self { prefix, ..self }
    }

    /// Returns a new FunctionCall with the given suffixes
    pub fn with_suffixes(self, suffixes: Vec<Suffix<P>>) -> Self {
        Self { suffixes, ..self }
    }
}

/// A function name when being declared as [`FunctionDeclaration`]
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[display(
    fmt = "{}{}{}",
    "names",
    "display_option(self.method_colon())",
    "display_option(self.method_name())"
)]
pub struct FunctionName<P: Plugin = DefaultPlugin> {
    names: Punctuated<TokenReference>,
    colon_name: Option<(TokenReference, TokenReference)>,

    plugin_info: <<P as Plugin>::FunctionNameMod as PluginMod<FunctionName<P>>>::NodeInfo,
}

impl<P: Plugin> FunctionName<P> {
    /// Creates a new FunctionName from the given list of names
    pub fn new(names: Punctuated<TokenReference>) -> Self {
        Self {
            names,
            colon_name: None,

            plugin_info: Default::default(),
        }
    }

    /// The colon between the name and the method, the `:` part of `function x:y() end`
    pub fn method_colon(&self) -> Option<&TokenReference> {
        Some(&self.colon_name.as_ref()?.0)
    }

    /// A method name if one exists, the `y` part of `function x:y() end`
    pub fn method_name(&self) -> Option<&TokenReference> {
        Some(&self.colon_name.as_ref()?.1)
    }

    /// Returns the punctuated sequence over the names used when defining the function.
    /// This is the `x.y.z` part of `function x.y.z() end`
    pub fn names(&self) -> &Punctuated<TokenReference> {
        &self.names
    }

    /// Returns a new FunctionName with the given names
    pub fn with_names(self, names: Punctuated<TokenReference>) -> Self {
        Self { names, ..self }
    }

    /// Returns a new FunctionName with the given method name
    /// The first token is the colon, and the second token is the method name itself
    pub fn with_method(self, method: Option<(TokenReference, TokenReference)>) -> Self {
        Self {
            colon_name: method,
            ..self
        }
    }
}

/// A normal function declaration, supports simple declarations like `function x() end`
/// as well as complicated declarations such as `function x.y.z:a() end`
#[derive(Clone, Debug, Display, PartialEq, Node, Visit)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(
    not(feature = "roblox"),
    display(fmt = "{}{}{}", "function_token", "name", "body")
)]
#[cfg_attr(
    feature = "roblox",
    display(fmt = "{}{}{}", "function_token", "name", "body")
)]
pub struct FunctionDeclaration<P: Plugin = DefaultPlugin> {
    function_token: TokenReference,
    name: FunctionName<P>,
    body: FunctionBody<P>,

    plugin_info:
        <<P as Plugin>::FunctionDeclarationMod as PluginMod<FunctionDeclaration<P>>>::NodeInfo,
}

impl<P: Plugin> FunctionDeclaration<P> {
    /// Creates a new FunctionDeclaration from the given name
    pub fn new(name: FunctionName<P>) -> Self {
        Self {
            function_token: TokenReference::symbol("function ").unwrap(),
            name,
            body: FunctionBody::new(),

            plugin_info: Default::default(),
        }
    }

    /// The `function` token
    pub fn function_token(&self) -> &TokenReference {
        &self.function_token
    }

    /// The body of the function
    pub fn body(&self) -> &FunctionBody<P> {
        &self.body
    }

    /// The name of the function
    pub fn name(&self) -> &FunctionName<P> {
        &self.name
    }

    /// Returns a new FunctionDeclaration with the given `function` token
    pub fn with_function_token(self, function_token: TokenReference) -> Self {
        Self {
            function_token,
            ..self
        }
    }

    /// Returns a new FunctionDeclaration with the given function name
    pub fn with_name(self, name: FunctionName<P>) -> Self {
        Self { name, ..self }
    }

    /// Returns a new FunctionDeclaration with the given function body
    pub fn with_body(self, body: FunctionBody<P>) -> Self {
        Self { body, ..self }
    }
}

make_op!(BinOp,
    #[doc = "Operators that require two operands, such as X + Y or X - Y"]
    #[visit(skip_visit_self)]
    {
        And,
        Caret,
        GreaterThan,
        GreaterThanEqual,
        LessThan,
        LessThanEqual,
        Minus,
        Or,
        Percent,
        Plus,
        Slash,
        Star,
        TildeEqual,
        TwoDots,
        TwoEqual,
    }
);

impl BinOp {
    /// The precedence of the operator, from a scale of 1 to 8. The larger the number, the higher the precedence.
    /// See more at http://www.lua.org/manual/5.1/manual.html#2.5.6
    pub fn precedence(&self) -> u8 {
        match *self {
            BinOp::Caret(_) => 8,
            BinOp::Star(_) | BinOp::Slash(_) | BinOp::Percent(_) => 6,
            BinOp::Plus(_) | BinOp::Minus(_) => 5,
            BinOp::TwoDots(_) => 4,
            BinOp::GreaterThan(_)
            | BinOp::LessThan(_)
            | BinOp::GreaterThanEqual(_)
            | BinOp::LessThanEqual(_)
            | BinOp::TildeEqual(_)
            | BinOp::TwoEqual(_) => 3,
            BinOp::And(_) => 2,
            BinOp::Or(_) => 1,
        }
    }

    /// Whether the operator is right associative. If not, it is left associative.
    /// See more at https://www.lua.org/pil/3.5.html
    pub fn is_right_associative(&self) -> bool {
        matches!(*self, BinOp::Caret(_) | BinOp::TwoDots(_))
    }

    /// The token associated with the operator
    pub fn token(&self) -> &TokenReference {
        match self {
            BinOp::And(token)
            | BinOp::Caret(token)
            | BinOp::GreaterThan(token)
            | BinOp::GreaterThanEqual(token)
            | BinOp::LessThan(token)
            | BinOp::LessThanEqual(token)
            | BinOp::Minus(token)
            | BinOp::Or(token)
            | BinOp::Percent(token)
            | BinOp::Plus(token)
            | BinOp::Slash(token)
            | BinOp::Star(token)
            | BinOp::TildeEqual(token)
            | BinOp::TwoDots(token)
            | BinOp::TwoEqual(token) => token,
        }
    }
}

make_op!(UnOp,
    #[doc = "Operators that require just one operand, such as #X"]
    {
        Minus,
        Not,
        Hash,
    }
);

impl UnOp {
    /// The precedence of the operator, from a scale of 1 to 8. The larger the number, the higher the precedence.
    /// See more at http://www.lua.org/manual/5.1/manual.html#2.5.6
    pub fn precedence(&self) -> u8 {
        7
    }

    /// The token associated with the operator
    pub fn token(&self) -> &TokenReference {
        match self {
            UnOp::Minus(token) | UnOp::Not(token) | UnOp::Hash(token) => token,
        }
    }
}

/// An error that occurs when creating the ast *after* tokenizing
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub enum AstError {
    /// There were no tokens passed, which shouldn't happen normally
    Empty,
    /// Tokens passed had no end of file token, which shouldn't happen normally
    NoEof,
    /// An unexpected token, the most likely scenario when getting an AstError
    UnexpectedToken {
        /// The token that caused the error
        token: Token,
        /// Any additional information that could be provided for debugging
        additional: Option<Cow<'static, str>>,
    },
}

impl fmt::Display for AstError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AstError::Empty => write!(formatter, "tokens passed was empty, which shouldn't happen normally"),
            AstError::NoEof => write!(formatter, "tokens passed had no eof token, which shouldn't happen normally"),
            AstError::UnexpectedToken { token, additional } => write!(
                formatter,
                "unexpected token `{}`. (starting from line {}, character {} and ending on line {}, character {}){}",
                token,
                token.start_position().line(),
                token.start_position().character(),
                token.end_position().line(),
                token.end_position().character(),
                match additional {
                    Some(additional) => format!("\nadditional information: {}", additional),
                    None => String::new(),
                }
            )
        }
    }
}

impl std::error::Error for AstError {}

/// An abstract syntax tree, contains all the nodes used in the code
#[derive(Clone, Debug)]
pub struct Ast<P: Plugin = DefaultPlugin> {
    pub(crate) nodes: Block<P>,
    pub(crate) eof: TokenReference,
}

impl<P: Plugin> Ast<P> {
    /// Create an Ast from the passed tokens. You probably want [`parse`](crate::parse)
    ///
    /// # Errors
    ///
    /// If the tokens passed are impossible to get through normal tokenization,
    /// an error of Empty (if the vector is empty) or NoEof (if there is no eof token)
    /// will be returned.
    ///
    /// More likely, if the tokens pass are invalid Lua 5.1 code, an
    /// UnexpectedToken error will be returned.
    pub fn from_tokens(tokens: Vec<Token>) -> Result<Ast<P>, AstError> {
        if *tokens.last().ok_or(AstError::Empty)?.token_type() != TokenType::Eof {
            Err(AstError::NoEof)
        } else {
            let mut tokens = extract_token_references(tokens);
            let mut state: ParserState<P> = ParserState::new(&tokens);

            if tokens
                .iter()
                .filter(|token| !token.token_type().is_trivia())
                .count()
                == 1
            {
                // Entirely comments/whitespace
                return Ok(Ast {
                    nodes: Block::new(),
                    eof: tokens.pop().expect(
                        "(internal full-moon error) No EOF in tokens after checking for EOF.",
                    ),
                });
            }

            // ParserState has to have at least 2 tokens, the last being an EOF, thus unwrap() can't fail
            if state.peek().token_type().is_trivia() {
                state = state.advance().unwrap();
            }

            match parsers::ParseBlock.parse(state) {
                Ok((state, block)) => {
                    if state.index == tokens.len() - 1 {
                        Ok(Ast {
                            nodes: block,
                            eof: tokens.pop().expect(
                                "(internal full-moon error) No EOF in tokens after checking for EOF."
                            ),
                        })
                    } else {
                        Err(AstError::UnexpectedToken {
                            token: state.peek().token.clone(),
                            additional: Some(Cow::Borrowed("leftover token")),
                        })
                    }
                }

                Err(InternalAstError::NoMatch) => Err(AstError::UnexpectedToken {
                    token: state.peek().token.clone(),
                    additional: None,
                }),

                Err(InternalAstError::UnexpectedToken { token, additional }) => {
                    Err(AstError::UnexpectedToken {
                        token: token.token,
                        additional,
                    })
                }
            }
        }
    }

    /// Returns a new Ast with the given nodes
    pub fn with_nodes(self, nodes: Block<P>) -> Self {
        Self { nodes, ..self }
    }

    /// Returns a new Ast with the given EOF token
    pub fn with_eof(self, eof: TokenReference) -> Self {
        Self { eof, ..self }
    }

    /// The entire code of the function
    ///
    /// ```rust
    /// # fn main() -> Result<(), Box<std::error::Error>> {
    /// assert_eq!(full_moon::parse("local x = 1; local y = 2")?.nodes().stmts().count(), 2);
    /// # Ok(())
    /// # }
    /// ```
    pub fn nodes(&self) -> &Block<P> {
        &self.nodes
    }

    /// The entire code of the function, but mutable
    pub fn nodes_mut(&mut self) -> &mut Block<P> {
        &mut self.nodes
    }

    /// The EOF token at the end of every Ast
    pub fn eof(&self) -> &TokenReference {
        &self.eof
    }
}

/// Extracts leading and trailing trivia from tokens
pub(crate) fn extract_token_references(mut tokens: Vec<Token>) -> Vec<TokenReference> {
    let mut references = Vec::new();
    let (mut leading_trivia, mut trailing_trivia) = (Vec::new(), Vec::new());
    let mut tokens = tokens.drain(..).peekable();

    while let Some(token) = tokens.next() {
        if token.token_type().is_trivia() {
            leading_trivia.push(token);
        } else {
            while let Some(token) = tokens.peek() {
                if token.token_type().is_trivia() {
                    // Take all trivia up to and including the newline character. If we see a newline character
                    // we should break once we have taken it in.
                    let should_break =
                        if let TokenType::Whitespace { ref characters } = &*token.token_type() {
                            // Use contains in order to tolerate \r\n line endings and mixed whitespace tokens
                            characters.contains('\n')
                        } else {
                            false
                        };

                    trailing_trivia.push(tokens.next().unwrap());

                    if should_break {
                        break;
                    }
                } else {
                    break;
                }
            }

            references.push(TokenReference {
                leading_trivia: leading_trivia.drain(..).collect(),
                trailing_trivia: trailing_trivia.drain(..).collect(),
                token,
            });
        }
    }

    references
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, print, tokenizer::tokens, visitors::VisitorMut};

    #[test]
    fn test_extract_token_references() {
        let tokens = tokens("print(1)\n-- hello world\nlocal foo -- this is the word foo").unwrap();

        let references = extract_token_references(tokens);
        assert_eq!(references.len(), 7);

        assert!(references[0].trailing_trivia.is_empty());
        assert_eq!(references[0].token.to_string(), "print");
        assert!(references[0].leading_trivia.is_empty());

        assert!(references[1].trailing_trivia.is_empty());
        assert_eq!(references[1].token.to_string(), "(");
        assert!(references[1].leading_trivia.is_empty());

        assert!(references[2].trailing_trivia.is_empty());
        assert_eq!(references[2].token.to_string(), "1");
        assert!(references[2].leading_trivia.is_empty());

        assert_eq!(references[3].trailing_trivia[0].to_string(), "\n");
        assert_eq!(references[3].token.to_string(), ")");
        assert!(references[3].leading_trivia.is_empty());

        assert_eq!(
            references[4].leading_trivia[0].to_string(),
            "-- hello world",
        );

        assert_eq!(references[4].leading_trivia[1].to_string(), "\n");
        assert_eq!(references[4].token.to_string(), "local");
        assert_eq!(references[4].trailing_trivia[0].to_string(), " ");
    }

    #[test]
    fn test_with_eof_safety() {
        let new_ast = {
            let ast = parse("local foo = 1").unwrap();
            let eof = ast.eof().clone();
            ast.with_eof(eof)
        };

        print(&new_ast);
    }

    #[test]
    fn test_with_nodes_safety() {
        let new_ast = {
            let ast = parse("local foo = 1").unwrap();
            let nodes = ast.nodes().clone();
            ast.with_nodes(nodes)
        };

        print(&new_ast);
    }

    #[test]
    fn test_with_visitor_safety() {
        let new_ast = {
            let ast = parse("local foo = 1").unwrap();

            struct SyntaxRewriter;
            impl VisitorMut for SyntaxRewriter {
                fn visit_token(&mut self, token: Token) -> Token {
                    token
                }
            }

            SyntaxRewriter.visit_ast(ast)
        };

        print(&new_ast);
    }

    // Tests AST nodes with new methods that call unwrap
    #[test]
    fn test_new_validity() {
        let token: TokenReference = TokenReference::new(
            Vec::new(),
            Token::new(TokenType::Identifier {
                identifier: "foo".into(),
            }),
            Vec::new(),
        );

        let expression: Expression = Expression::Value {
            value: Box::new(Value::Var(Var::Name(token.clone()))),
            #[cfg(feature = "roblox")]
            type_assertion: None,
        };

        // PLUGIN TODO: `new` should not require a generic.
        // Rust has a similar problem with HashMap::new() and RandomState.
        // Creating a node with a plugin that's non-default should be a separate method.

        let _: Assignment = Assignment::new(Punctuated::new(), Punctuated::new());
        let _: Do = Do::new();
        let _: ElseIf = ElseIf::new(expression.clone());
        let _: FunctionBody = FunctionBody::new();
        let _: FunctionCall = FunctionCall::new(Prefix::Name(token.clone()));
        let _: FunctionDeclaration = FunctionDeclaration::new(FunctionName::new(Punctuated::new()));
        let _: GenericFor = GenericFor::new(Punctuated::new(), Punctuated::new());
        let _: If = If::new(expression.clone());
        let _: LocalAssignment = LocalAssignment::new(Punctuated::new());
        let _: LocalFunction = LocalFunction::new(token.clone());
        let _: MethodCall = MethodCall::new(
            token.clone(),
            FunctionArgs::Parentheses {
                arguments: Punctuated::new(),
                parentheses: ContainedSpan::new(token.clone(), token.clone()),
            },
        );
        let _: NumericFor = NumericFor::new(token, expression.clone(), expression.clone());
        let _: Repeat = Repeat::new(expression.clone());
        let _: Return = Return::new();
        let _: TableConstructor = TableConstructor::new();
        let _: While = While::new(expression);
    }
}
