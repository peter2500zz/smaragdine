//! Implementation detail of smaragdine's optional command macros.
//!
//! Use `smaragdine::{command, commands}`. Those thin declarative entry points
//! pass their hygienic `$crate` path to this parser, including when the library
//! is renamed or re-exported. No Cargo manifest lookup or runtime dependency is
//! needed. The parser preserves user expression spans; rustc performs all type
//! and method resolution on the generated builder calls.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{quote, quote_spanned};
use syn::{
    Expr, Ident, Token, braced, bracketed,
    ext::IdentExt,
    parse::{Parse, ParseStream},
    spanned::Spanned,
};

#[cfg(test)]
mod tests;

const HANDLER_SYNTAX: &str =
    "expected `run: handler;`, `run sync: handler;`, or `run async: handler;`";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Sync,
    Async,
}

impl Parse for Mode {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.call(Ident::parse_any)?;
        match ident.to_string().as_str() {
            "sync" => Ok(Self::Sync),
            "async" if cfg!(feature = "async") => Ok(Self::Async),
            "async" => Err(syn::Error::new(
                ident.span(),
                "async command registration requires the async feature; enable smaragdine's `async` feature or use sync registration",
            )),
            _ => Err(syn::Error::new(
                ident.span(),
                format!("unknown command mode `{ident}`; expected `sync` or `async`"),
            )),
        }
    }
}

fn expression(input: ParseStream, kind: &str) -> syn::Result<Expr> {
    input.parse().map_err(|error: syn::Error| {
        let message = if kind == "node" {
            format!("invalid node expression; expected `node => {{ ... }}`: {error}")
        } else {
            format!("invalid {kind} expression: {error}")
        };
        syn::Error::new(error.span(), message)
    })
}

fn semicolon(input: ParseStream, kind: &str) -> syn::Result<()> {
    if !input.peek(Token![;]) {
        return Err(input.error(format!("missing `;` after {kind}; use `;`, not `,`")));
    }
    input.parse::<Token![;]>()?;
    Ok(())
}

struct Handler {
    mode: Option<Mode>,
    expression: Expr,
}

impl Parse for Handler {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mode = if input.peek(Token![:]) {
            None
        } else if input.peek(Ident::peek_any) {
            Some(input.parse()?)
        } else {
            return Err(input.error(HANDLER_SYNTAX));
        };
        if !input.peek(Token![:]) {
            return Err(input.error(HANDLER_SYNTAX));
        }
        input.parse::<Token![:]>()?;
        if input.is_empty() || input.peek(Token![;]) {
            return Err(input.error("missing handler after `run:`; expected `run: handler;`"));
        }
        let expression = expression(input, "handler")?;
        semicolon(input, "handler")?;
        Ok(Self { mode, expression })
    }
}

struct Node {
    expression: Expr,
    handler: Option<Handler>,
    children: Vec<Node>,
}

fn run_entry(input: ParseStream) -> bool {
    let ahead = input.fork();
    let Ok(ident) = ahead.call(Ident::parse_any) else {
        return false;
    };
    // `=` also peeks successfully at the start of `=>` and `==` in syn.
    // A builder named `run`, including `run::factory()` and `run.method()`, is
    // still an ordinary expression, not a reserved entry.
    ident == "run"
        && !ahead.peek(Token![::])
        && !ahead.peek(Token![=>])
        && !ahead.peek(Token![==])
        && (ahead.peek(Token![:]) || ahead.peek(Token![=]) || ahead.peek(Ident::peek_any))
}

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let expression = expression(input, "node")?;
        if !input.peek(Token![=>]) {
            return Err(input.error("expected `=>` between node expression and `{ ... }`"));
        }
        input.parse::<Token![=>]>()?;
        let body;
        braced!(body in input);
        let mut handler = None;
        let mut children = Vec::new();
        while !body.is_empty() {
            if run_entry(&body) {
                let run = body.call(Ident::parse_any)?;
                if handler.is_some() {
                    return Err(syn::Error::new(
                        run.span(),
                        "duplicate `run`; a node may have only one handler",
                    ));
                }
                if !children.is_empty() {
                    return Err(syn::Error::new(
                        run.span(),
                        "`run` must precede child nodes; move the handler before the first child",
                    ));
                }
                handler = Some(body.parse()?);
            } else {
                let ahead = body.fork();
                if let Ok(ident) = ahead.call(Ident::parse_any)
                    && ahead.peek(Token![:])
                    && !ahead.peek(Token![::])
                {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown node entry `{ident}:`; use `run: handler;` to attach a handler"
                        ),
                    ));
                }
                children.push(body.parse()?);
                semicolon(&body, "child node")?;
            }
        }
        Ok(Self {
            expression,
            handler,
            children,
        })
    }
}

impl Node {
    fn expand(&self, library: &Tokens, default: Mode) -> Tokens {
        let expression = &self.expression;
        let span = expression.span();
        let mut initial = quote!(#expression);
        if let Some(handler) = &self.handler {
            let handler_expression = &handler.expression;
            initial = match handler.mode.unwrap_or(default) {
                Mode::Sync => quote_spanned!(span=>
                    #library::brigadier::builder::CommandBuilder::executes(#initial, #handler_expression)
                ),
                Mode::Async => quote_spanned!(span=>
                    #library::brigadier::builder::CommandBuilder::executes_async(#initial, #handler_expression)
                ),
            };
        }
        let local = Ident::new("__smaragdine_node", Span::mixed_site());
        // Sibling parsing and expansion use iteration, not macro recursion.
        let children = self
            .children
            .iter()
            .map(|node| node.expand(library, default));
        quote_spanned!(span=> {
            let #local = #initial;
            #(let #local = #library::brigadier::builder::CommandBuilder::then(#local, #children);)*
            #local
        })
    }
}

fn library(input: ParseStream) -> syn::Result<Tokens> {
    let path;
    bracketed!(path in input);
    let tokens = path.parse()?;
    input.parse::<Token![;]>()?;
    Ok(tokens)
}

struct Command {
    library: Tokens,
    mode: Mode,
    node: Node,
}

impl Parse for Command {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let library = library(input)?;
        let mode = if input.peek(Ident::peek_any) && input.peek2(Token![;]) {
            let mode = input.parse()?;
            input.parse::<Token![;]>()?;
            mode
        } else {
            Mode::Sync
        };
        Ok(Self {
            library,
            mode,
            node: input.parse()?,
        })
    }
}

struct Commands {
    library: Tokens,
    receiver: Expr,
    mode: Mode,
    nodes: Vec<Node>,
}

impl Parse for Commands {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let library = library(input)?;
        let receiver = expression(input, "dispatcher")?;
        input.parse::<Token![,]>()?;
        // Validate the selected mode even for empty trees or local overrides.
        let mode = if input.peek(syn::token::Brace) {
            Mode::Sync
        } else {
            input.parse()?
        };
        let body;
        braced!(body in input);
        let mut nodes = Vec::new();
        while !body.is_empty() {
            if run_entry(&body) {
                return Err(body.error(
                    "`run` belongs inside a node body; expected `node => { run: handler; };`",
                ));
            }
            nodes.push(body.parse()?);
            semicolon(&body, "command node")?;
        }
        Ok(Self {
            library,
            receiver,
            mode,
            nodes,
        })
    }
}

impl Commands {
    fn expand(&self) -> Tokens {
        let nodes = self
            .nodes
            .iter()
            .map(|node| node.expand(&self.library, self.mode));
        let receiver = &self.receiver;
        // Match the original receiver semantics: preserve two-phase borrowing
        // for an identifier; evaluate a computed receiver once, including when
        // the tree is empty. Do not unwrap forwarded expression fragments.
        if matches!(receiver, Expr::Path(path) if path.qself.is_none() && path.path.get_ident().is_some())
        {
            quote!({ #(#receiver.register(#nodes);)* })
        } else {
            let local = Ident::new("__smaragdine_dispatcher", Span::mixed_site());
            // The receiver can be a binary expression. Parentheses preserve
            // the opaque `$dispatcher:expr` precedence of the previous macro.
            quote!({ let #local = &mut (#receiver); #(#local.register(#nodes);)* })
        }
    }
}

/// Internal entry point; invoke `smaragdine::command!` instead.
#[proc_macro]
pub fn command(tokens: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(tokens as Command);
    input.node.expand(&input.library, input.mode).into()
}

/// Internal entry point; invoke `smaragdine::commands!` instead.
#[proc_macro]
pub fn commands(tokens: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(tokens as Commands);
    input.expand().into()
}
